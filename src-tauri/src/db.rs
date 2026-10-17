//! SQLite index: schema, search and read queries.
//!
//! Full-text search uses FTS5's `trigram` tokenizer, which matches arbitrary substrings and so
//! works for Japanese without a morphological analyser. Trigrams need at least three characters;
//! shorter terms fall back to `LIKE` scans.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const HIT_START: char = '\u{E000}';
pub const HIT_END: char = '\u{E001}';
const SNIPPET_CONTEXT_CHARS: usize = 60;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sources (
    id INTEGER PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    agent_type TEXT NOT NULL,
    environment_type TEXT NOT NULL,
    distro_name TEXT,
    linux_home TEXT,
    root_path TEXT NOT NULL,
    last_scan_at TEXT,
    status TEXT NOT NULL DEFAULT 'pending'
);
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    project_path TEXT NOT NULL,
    display_name TEXT NOT NULL,
    UNIQUE (source_id, project_path)
);
-- Client/case tags are keyed by path, not project row, so they survive index deletion.
CREATE TABLE IF NOT EXISTS project_tags (
    project_path TEXT PRIMARY KEY,
    client_tag TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    external_session_id TEXT,
    title TEXT NOT NULL,
    cwd TEXT,
    git_branch TEXT,
    started_at TEXT,
    updated_at TEXT,
    file_path TEXT NOT NULL UNIQUE,
    file_size INTEGER NOT NULL,
    file_mtime INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    message_count INTEGER NOT NULL DEFAULT 0,
    bad_lines INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS sessions_updated ON sessions(updated_at);
CREATE INDEX IF NOT EXISTS sessions_external ON sessions(external_session_id);
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    sequence_no INTEGER NOT NULL,
    role TEXT NOT NULL,
    timestamp TEXT,
    searchable_text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_session ON messages(session_id, sequence_no);
CREATE TABLE IF NOT EXISTS scan_errors (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    occurred_at TEXT NOT NULL,
    file_path TEXT,
    error_summary TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    searchable_text, content='messages', content_rowid='id', tokenize='trigram'
);
CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, searchable_text) VALUES (new.id, new.searchable_text);
END;
CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, searchable_text)
    VALUES ('delete', old.id, old.searchable_text);
END;

CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts USING fts5(
    title, project_path, tokenize='trigram'
);
CREATE TRIGGER IF NOT EXISTS sessions_ai AFTER INSERT ON sessions BEGIN
    INSERT INTO sessions_fts(rowid, title, project_path)
    VALUES (new.id, new.title, coalesce(new.cwd, ''));
END;
CREATE TRIGGER IF NOT EXISTS sessions_ad AFTER DELETE ON sessions BEGIN
    DELETE FROM sessions_fts WHERE rowid = old.id;
END;
"#;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON; \
         PRAGMA busy_timeout = 5000;",
    )?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

/// Removes every indexed copy of history content. Tags and settings are kept.
pub fn clear_index(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DELETE FROM messages; DELETE FROM sessions; DELETE FROM projects; \
         DELETE FROM scan_errors; DELETE FROM sources; \
         INSERT INTO messages_fts(messages_fts) VALUES ('rebuild'); \
         DELETE FROM sessions_fts;",
    )?;
    conn.execute_batch("VACUUM;")
}

#[derive(Debug, Default, Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub source_keys: Vec<String>,
    pub project_id: Option<i64>,
    pub client_tag: Option<String>,
    /// Inclusive lower bound on `updated_at` (RFC 3339).
    pub updated_from: Option<String>,
    /// Exclusive upper bound on `updated_at` (RFC 3339).
    pub updated_to: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub session_id: i64,
    pub external_session_id: Option<String>,
    pub title: String,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub source_key: String,
    pub environment_type: String,
    pub distro_name: Option<String>,
    pub project_id: Option<i64>,
    pub project_path: Option<String>,
    pub project_name: Option<String>,
    pub client_tag: Option<String>,
    pub git_branch: Option<String>,
    pub message_count: i64,
    pub hit_count: i64,
    /// Other indexed files holding the same session id (e.g. a backup copy).
    pub duplicates: i64,
    /// Excerpt with matches wrapped in `HIT_START` / `HIT_END`.
    pub snippet: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub total: usize,
}

/// Per-session match info for one search term: first matching message (if the match was in the
/// conversation rather than the title/path) and the number of matching messages.
type TermHits = HashMap<i64, (Option<i64>, i64)>;

fn terms_of(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_string).collect()
}

fn fts_phrase(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

fn like_pattern(term: &str) -> String {
    let escaped = term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

fn term_hits(conn: &Connection, term: &str) -> rusqlite::Result<TermHits> {
    let mut hits: TermHits = HashMap::new();
    let use_fts = term.chars().count() >= 3;
    let (msg_sql, sess_sql, arg) = if use_fts {
        (
            "SELECT m.session_id, MIN(m.id), COUNT(*) FROM messages_fts \
             JOIN messages m ON m.id = messages_fts.rowid \
             WHERE messages_fts MATCH ?1 GROUP BY m.session_id",
            "SELECT rowid FROM sessions_fts WHERE sessions_fts MATCH ?1",
            fts_phrase(term),
        )
    } else {
        (
            "SELECT session_id, MIN(id), COUNT(*) FROM messages \
             WHERE searchable_text LIKE ?1 ESCAPE '\\' GROUP BY session_id",
            "SELECT id FROM sessions WHERE title LIKE ?1 ESCAPE '\\' \
             OR coalesce(cwd, '') LIKE ?1 ESCAPE '\\'",
            like_pattern(term),
        )
    };
    let mut stmt = conn.prepare(msg_sql)?;
    let rows = stmt.query_map([&arg], |r| Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?)))?;
    for row in rows {
        let (sid, mid, n) = row?;
        hits.insert(sid, (Some(mid), n));
    }
    let mut stmt = conn.prepare(sess_sql)?;
    let rows = stmt.query_map([&arg], |r| r.get::<_, i64>(0))?;
    for sid in rows {
        hits.entry(sid?).or_insert((None, 0));
    }
    Ok(hits)
}

pub fn search(conn: &Connection, p: &SearchParams) -> rusqlite::Result<SearchResult> {
    let terms = terms_of(&p.query);
    let limit = p.limit.unwrap_or(200).clamp(1, 1000) as usize;

    // Sessions matching every term, anywhere in the conversation, title or project path.
    let mut matched: Option<HashMap<i64, (Option<i64>, i64)>> = None;
    for term in &terms {
        let hits = term_hits(conn, term)?;
        matched = Some(match matched {
            None => hits,
            Some(prev) => prev
                .into_iter()
                .filter_map(|(sid, (mid, n))| {
                    hits.get(&sid).map(|(mid2, n2)| (sid, (mid.or(*mid2), n + n2)))
                })
                .collect(),
        });
    }

    let mut sql = String::from(
        "SELECT s.id, s.external_session_id, s.title, s.started_at, s.updated_at, \
                src.key, src.environment_type, src.distro_name, \
                p.id, p.project_path, p.display_name, t.client_tag, s.git_branch, s.message_count, \
                (SELECT COUNT(*) FROM sessions d WHERE d.external_session_id = s.external_session_id \
                   AND d.id <> s.id) \
         FROM sessions s \
         JOIN sources src ON src.id = s.source_id \
         LEFT JOIN projects p ON p.id = s.project_id \
         LEFT JOIN project_tags t ON t.project_path = p.project_path \
         WHERE 1 = 1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if !p.source_keys.is_empty() {
        let marks = vec!["?"; p.source_keys.len()].join(",");
        sql.push_str(&format!(" AND src.key IN ({marks})"));
        for k in &p.source_keys {
            args.push(Box::new(k.clone()));
        }
    }
    if let Some(pid) = p.project_id {
        sql.push_str(" AND s.project_id = ?");
        args.push(Box::new(pid));
    }
    if let Some(tag) = p.client_tag.as_ref().filter(|t| !t.is_empty()) {
        sql.push_str(" AND t.client_tag = ?");
        args.push(Box::new(tag.clone()));
    }
    if let Some(from) = p.updated_from.as_ref().filter(|t| !t.is_empty()) {
        sql.push_str(" AND s.updated_at >= ?");
        args.push(Box::new(from.clone()));
    }
    if let Some(to) = p.updated_to.as_ref().filter(|t| !t.is_empty()) {
        sql.push_str(" AND s.updated_at < ?");
        args.push(Box::new(to.clone()));
    }
    sql.push_str(" ORDER BY s.updated_at DESC");

    let mut stmt = conn.prepare(&sql)?;
    let arg_refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
    let rows = stmt.query_map(arg_refs.as_slice(), |r| {
        Ok(SearchHit {
            session_id: r.get(0)?,
            external_session_id: r.get(1)?,
            title: r.get(2)?,
            started_at: r.get(3)?,
            updated_at: r.get(4)?,
            source_key: r.get(5)?,
            environment_type: r.get(6)?,
            distro_name: r.get(7)?,
            project_id: r.get(8)?,
            project_path: r.get(9)?,
            project_name: r.get(10)?,
            client_tag: r.get(11)?,
            git_branch: r.get(12)?,
            message_count: r.get(13)?,
            duplicates: r.get(14)?,
            hit_count: 0,
            snippet: None,
        })
    })?;

    let mut hits = Vec::new();
    let mut total = 0;
    for row in rows {
        let mut hit = row?;
        if let Some(m) = &matched {
            let Some((mid, n)) = m.get(&hit.session_id) else {
                continue;
            };
            hit.hit_count = *n;
            if hits.len() < limit {
                hit.snippet = match mid {
                    Some(mid) => message_text(conn, *mid)?.map(|t| make_snippet(&t, &terms)),
                    None => None,
                };
            }
        }
        total += 1;
        if hits.len() < limit {
            hits.push(hit);
        }
    }
    Ok(SearchResult { hits, total })
}

fn message_text(conn: &Connection, id: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT searchable_text FROM messages WHERE id = ?1", [id], |r| r.get(0))
        .optional()
}

/// Excerpt around the first match, with every match marked. Matching is ASCII-case-insensitive,
/// which keeps byte offsets identical between the lowered and original text.
pub fn make_snippet(text: &str, terms: &[String]) -> String {
    let lower = text.to_ascii_lowercase();
    let needles: Vec<String> = terms.iter().map(|t| t.to_ascii_lowercase()).collect();
    let first = needles.iter().filter_map(|n| lower.find(n.as_str())).min().unwrap_or(0);

    let start = text[..first]
        .char_indices()
        .rev()
        .nth(SNIPPET_CONTEXT_CHARS - 1)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let end = text[first..]
        .char_indices()
        .nth(SNIPPET_CONTEXT_CHARS * 3)
        .map(|(i, _)| first + i)
        .unwrap_or(text.len());

    let window = &text[start..end];
    let lwindow = &lower[start..end];
    let mut marks: Vec<(usize, usize)> = Vec::new();
    for n in needles.iter().filter(|n| !n.is_empty()) {
        let mut from = 0;
        while let Some(pos) = lwindow[from..].find(n.as_str()) {
            let s = from + pos;
            marks.push((s, s + n.len()));
            from = s + n.len();
        }
    }
    marks.sort();
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    let mut cursor = 0;
    for (s, e) in marks {
        if s < cursor {
            continue;
        }
        out.push_str(&window[cursor..s]);
        out.push(HIT_START);
        out.push_str(&window[s..e]);
        out.push(HIT_END);
        cursor = e;
    }
    out.push_str(&window[cursor..]);
    if end < text.len() {
        out.push('…');
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Serialize)]
pub struct MessageView {
    pub sequence_no: i64,
    pub role: String,
    pub timestamp: Option<String>,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct SessionDetail {
    pub session_id: i64,
    pub external_session_id: Option<String>,
    pub title: String,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub file_path: String,
    pub source_key: String,
    pub environment_type: String,
    pub distro_name: Option<String>,
    pub linux_home: Option<String>,
    pub client_tag: Option<String>,
    pub bad_lines: i64,
    pub messages: Vec<MessageView>,
}

pub fn session_detail(conn: &Connection, id: i64) -> rusqlite::Result<Option<SessionDetail>> {
    let detail = conn
        .query_row(
            "SELECT s.id, s.external_session_id, s.title, s.cwd, s.git_branch, s.started_at, \
                    s.updated_at, s.file_path, src.key, src.environment_type, src.distro_name, \
                    src.linux_home, t.client_tag, s.bad_lines \
             FROM sessions s JOIN sources src ON src.id = s.source_id \
             LEFT JOIN projects p ON p.id = s.project_id \
             LEFT JOIN project_tags t ON t.project_path = p.project_path \
             WHERE s.id = ?1",
            [id],
            |r| {
                Ok(SessionDetail {
                    session_id: r.get(0)?,
                    external_session_id: r.get(1)?,
                    title: r.get(2)?,
                    cwd: r.get(3)?,
                    git_branch: r.get(4)?,
                    started_at: r.get(5)?,
                    updated_at: r.get(6)?,
                    file_path: r.get(7)?,
                    source_key: r.get(8)?,
                    environment_type: r.get(9)?,
                    distro_name: r.get(10)?,
                    linux_home: r.get(11)?,
                    client_tag: r.get(12)?,
                    bad_lines: r.get(13)?,
                    messages: Vec::new(),
                })
            },
        )
        .optional()?;
    let Some(mut detail) = detail else {
        return Ok(None);
    };
    let mut stmt = conn.prepare(
        "SELECT sequence_no, role, timestamp, searchable_text FROM messages \
         WHERE session_id = ?1 ORDER BY sequence_no",
    )?;
    detail.messages = stmt
        .query_map([id], |r| {
            Ok(MessageView { sequence_no: r.get(0)?, role: r.get(1)?, timestamp: r.get(2)?, text: r.get(3)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(detail))
}

#[derive(Debug, Serialize)]
pub struct ProjectView {
    pub id: i64,
    pub source_key: String,
    pub project_path: String,
    pub display_name: String,
    pub client_tag: Option<String>,
    pub session_count: i64,
    pub last_updated_at: Option<String>,
}

pub fn list_projects(conn: &Connection) -> rusqlite::Result<Vec<ProjectView>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, src.key, p.project_path, p.display_name, t.client_tag, \
                COUNT(s.id), MAX(s.updated_at) \
         FROM projects p JOIN sources src ON src.id = p.source_id \
         LEFT JOIN sessions s ON s.project_id = p.id \
         LEFT JOIN project_tags t ON t.project_path = p.project_path \
         GROUP BY p.id ORDER BY MAX(s.updated_at) DESC",
    )?;
    stmt.query_map([], |r| {
        Ok(ProjectView {
            id: r.get(0)?,
            source_key: r.get(1)?,
            project_path: r.get(2)?,
            display_name: r.get(3)?,
            client_tag: r.get(4)?,
            session_count: r.get(5)?,
            last_updated_at: r.get(6)?,
        })
    })?
    .collect()
}

pub fn set_project_tag(conn: &Connection, project_path: &str, tag: Option<&str>) -> rusqlite::Result<()> {
    match tag.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => conn.execute(
            "INSERT INTO project_tags(project_path, client_tag) VALUES (?1, ?2) \
             ON CONFLICT(project_path) DO UPDATE SET client_tag = excluded.client_tag",
            params![project_path, t],
        )?,
        None => conn.execute("DELETE FROM project_tags WHERE project_path = ?1", [project_path])?,
    };
    Ok(())
}

pub fn client_tags(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT client_tag FROM project_tags ORDER BY client_tag")?;
    stmt.query_map([], |r| r.get(0))?.collect()
}

#[derive(Debug, Serialize, Clone)]
pub struct SourceStatus {
    pub key: String,
    pub root_path: String,
    pub last_scan_at: Option<String>,
    pub status: String,
    pub session_count: i64,
    pub recent_errors: Vec<String>,
    pub error_count: i64,
}

pub fn source_statuses(conn: &Connection) -> rusqlite::Result<HashMap<String, SourceStatus>> {
    let mut stmt = conn.prepare(
        "SELECT src.id, src.key, src.root_path, src.last_scan_at, src.status, \
                (SELECT COUNT(*) FROM sessions s WHERE s.source_id = src.id), \
                (SELECT COUNT(*) FROM scan_errors e WHERE e.source_id = src.id) \
         FROM sources src",
    )?;
    let rows: Vec<(i64, SourceStatus)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                SourceStatus {
                    key: r.get(1)?,
                    root_path: r.get(2)?,
                    last_scan_at: r.get(3)?,
                    status: r.get(4)?,
                    session_count: r.get(5)?,
                    error_count: r.get(6)?,
                    recent_errors: Vec::new(),
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut err_stmt = conn.prepare(
        "SELECT coalesce(file_path || ': ', '') || error_summary FROM scan_errors \
         WHERE source_id = ?1 ORDER BY id DESC LIMIT 5",
    )?;
    let mut out = HashMap::new();
    for (id, mut st) in rows {
        st.recent_errors = err_stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        out.insert(st.key.clone(), st);
    }
    Ok(out)
}

/// Keys of sources that have sessions in the index (used to populate filters).
pub fn indexed_source_keys(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT src.key FROM sources src JOIN sessions s ON s.source_id = src.id ORDER BY src.key",
    )?;
    let keys: HashSet<String> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let mut keys: Vec<String> = keys.into_iter().collect();
    keys.sort();
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_marks_matches_and_trims() {
        let text = format!("{}検索機能のテストです。Search here.{}", "あ".repeat(100), "い".repeat(300));
        let s = make_snippet(&text, &["検索".into(), "search".into()]);
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert!(s.contains("\u{E000}検索\u{E001}"));
        assert!(s.contains("\u{E000}Search\u{E001}"));
    }

    #[test]
    fn like_pattern_escapes_wildcards() {
        assert_eq!(like_pattern("50%_a"), "%50\\%\\_a%");
    }
}
