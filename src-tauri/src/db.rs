//! SQLite index: schema, search and read queries.
//!
//! Full-text search uses FTS5's `trigram` tokenizer, which matches arbitrary substrings and so
//! works for Japanese without a morphological analyser. Trigrams need at least three characters;
//! shorter terms fall back to `LIKE` scans.
//!
//! Two kinds of data live here: the *index* (copies of history content, rebuildable at any time)
//! and *user data* (tags, display names). User data is keyed by the agent's session id rather
//! than by row id, so it survives rescans and index deletion.

use crate::error::AppError;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const HIT_START: char = '\u{E000}';
pub const HIT_END: char = '\u{E001}';
const SNIPPET_CONTEXT_CHARS: usize = 60;

/// Bump when the index schema changes; older index tables are dropped and rebuilt on next scan.
const INDEX_SCHEMA_VERSION: i64 = 2;

const INDEX_TABLES_DROP: &str = r#"
DROP TRIGGER IF EXISTS messages_ai;
DROP TRIGGER IF EXISTS messages_ad;
DROP TRIGGER IF EXISTS sessions_ai;
DROP TRIGGER IF EXISTS sessions_ad;
DROP TABLE IF EXISTS messages_fts;
DROP TABLE IF EXISTS sessions_fts;
DROP TABLE IF EXISTS messages;
DROP TABLE IF EXISTS scan_errors;
DROP TABLE IF EXISTS sessions;
DROP TABLE IF EXISTS projects;
DROP TABLE IF EXISTS sources;
"#;

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
    code TEXT NOT NULL,
    detail TEXT
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

-- User data (kept across rescans and index deletion).
CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    color TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS session_tags (
    external_session_id TEXT NOT NULL,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (external_session_id, tag_id)
);
CREATE TABLE IF NOT EXISTS session_titles (
    external_session_id TEXT PRIMARY KEY,
    title TEXT NOT NULL
);
"#;

pub const DEFAULT_TAG_COLOR: &str = "#3b82f6";

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON; \
         PRAGMA busy_timeout = 5000;",
    )?;
    migrate(&conn)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version >= INDEX_SCHEMA_VERSION {
        return Ok(());
    }
    // v1 had per-project "client tags"; keep their names as tags before dropping the table.
    let had_project_tags: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'project_tags')",
        [],
        |r| r.get(0),
    )?;
    conn.execute_batch(SCHEMA_USER_TABLES)?;
    if had_project_tags {
        conn.execute(
            "INSERT OR IGNORE INTO tags(name, color) SELECT DISTINCT client_tag, ?1 FROM project_tags",
            [DEFAULT_TAG_COLOR],
        )?;
        conn.execute_batch("DROP TABLE project_tags;")?;
    }
    // The index is a cache of history files, so rebuilding it is always safe.
    conn.execute_batch(INDEX_TABLES_DROP)?;
    conn.execute_batch(&format!("PRAGMA user_version = {INDEX_SCHEMA_VERSION};"))
}

const SCHEMA_USER_TABLES: &str = r#"
CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    color TEXT NOT NULL
);
"#;

/// Removes every indexed copy of history content. Tags, display names and settings are kept.
pub fn clear_index(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DELETE FROM messages; DELETE FROM sessions; DELETE FROM projects; \
         DELETE FROM scan_errors; DELETE FROM sources; \
         INSERT INTO messages_fts(messages_fts) VALUES ('rebuild'); \
         DELETE FROM sessions_fts;",
    )?;
    conn.execute_batch("VACUUM;")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TagView {
    pub id: i64,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TagMode {
    /// Sessions must carry every selected tag.
    #[default]
    And,
    /// Sessions must carry at least one selected tag.
    Or,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    #[default]
    Updated,
    Started,
    Title,
    Project,
    Messages,
    /// Number of matching messages; only meaningful with a query.
    Hits,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SortDir {
    Asc,
    #[default]
    Desc,
}

#[derive(Debug, Default, Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    pub query: String,
    /// Any of these sources (empty = all).
    #[serde(default)]
    pub source_keys: Vec<String>,
    /// Any of these projects (empty = all).
    #[serde(default)]
    pub project_ids: Vec<i64>,
    #[serde(default)]
    pub tag_ids: Vec<i64>,
    #[serde(default)]
    pub tag_mode: TagMode,
    /// Inclusive lower bound on `updated_at` (RFC 3339).
    pub updated_from: Option<String>,
    /// Exclusive upper bound on `updated_at` (RFC 3339).
    pub updated_to: Option<String>,
    #[serde(default)]
    pub sort: SortField,
    #[serde(default)]
    pub sort_dir: SortDir,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub session_id: i64,
    pub external_session_id: Option<String>,
    /// Display title: the user's rename if any, else the agent's title. Empty means untitled.
    pub title: String,
    pub renamed: bool,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub source_key: String,
    pub environment_type: String,
    pub distro_name: Option<String>,
    pub project_id: Option<i64>,
    pub project_path: Option<String>,
    pub project_name: Option<String>,
    pub git_branch: Option<String>,
    pub message_count: i64,
    pub hit_count: i64,
    /// Other indexed files holding the same session id (e.g. a backup copy).
    pub duplicates: i64,
    pub tags: Vec<TagView>,
    /// Excerpt with matches wrapped in `HIT_START` / `HIT_END`.
    pub snippet: Option<String>,
    /// First matching message, used to build the snippet once the page is chosen.
    #[serde(skip)]
    first_hit_message: Option<i64>,
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
    let like = like_pattern(term);
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
            like.clone(),
        )
    };
    let mut stmt = conn.prepare(msg_sql)?;
    let rows = stmt.query_map([&arg], |r| Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?)))?;
    for row in rows {
        let (sid, mid, n) = row?;
        hits.insert(sid, (Some(mid), n));
    }
    let mut stmt = conn.prepare(sess_sql)?;
    for sid in stmt.query_map([&arg], |r| r.get::<_, i64>(0))? {
        hits.entry(sid?).or_insert((None, 0));
    }
    // User renames and tag names are few, so a LIKE scan is enough for any term length.
    let mut stmt = conn.prepare(
        "SELECT s.id FROM sessions s JOIN session_titles t ON t.external_session_id = s.external_session_id \
         WHERE t.title LIKE ?1 ESCAPE '\\' \
         UNION SELECT s.id FROM sessions s \
         JOIN session_tags st ON st.external_session_id = s.external_session_id \
         JOIN tags g ON g.id = st.tag_id WHERE g.name LIKE ?1 ESCAPE '\\'",
    )?;
    for sid in stmt.query_map([&like], |r| r.get::<_, i64>(0))? {
        hits.entry(sid?).or_insert((None, 0));
    }
    Ok(hits)
}

/// Tags of every tagged session, keyed by session id.
fn tags_by_session(conn: &Connection) -> rusqlite::Result<HashMap<String, Vec<TagView>>> {
    let mut stmt = conn.prepare(
        "SELECT st.external_session_id, g.id, g.name, g.color FROM session_tags st \
         JOIN tags g ON g.id = st.tag_id ORDER BY g.name COLLATE NOCASE",
    )?;
    let mut map: HashMap<String, Vec<TagView>> = HashMap::new();
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, TagView { id: r.get(1)?, name: r.get(2)?, color: r.get(3)? }))
    })? {
        let (ext, tag) = row?;
        map.entry(ext).or_default().push(tag);
    }
    Ok(map)
}

/// `ORDER BY` clause for a sort field. Only fixed strings are ever placed in the SQL.
fn order_by(sort: SortField, dir: SortDir) -> String {
    let d = if dir == SortDir::Asc { "ASC" } else { "DESC" };
    // Missing values sort last in either direction; ties fall back to newest first.
    match sort {
        SortField::Updated | SortField::Hits => format!("s.updated_at IS NULL, s.updated_at {d}"),
        SortField::Started => format!("s.started_at IS NULL, s.started_at {d}, s.updated_at DESC"),
        SortField::Title => format!(
            "coalesce(t.title, s.title) = '', coalesce(t.title, s.title) COLLATE NOCASE {d}, s.updated_at DESC"
        ),
        SortField::Project => format!("p.display_name IS NULL, p.display_name COLLATE NOCASE {d}, s.updated_at DESC"),
        SortField::Messages => format!("s.message_count {d}, s.updated_at DESC"),
    }
}

pub fn search(conn: &Connection, p: &SearchParams) -> rusqlite::Result<SearchResult> {
    let terms = terms_of(&p.query);
    let limit = p.limit.unwrap_or(200).clamp(1, 1000) as usize;

    // Sessions matching every term, anywhere in the conversation, title, path or tags.
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
        "SELECT s.id, s.external_session_id, coalesce(t.title, s.title), t.title IS NOT NULL, \
                s.started_at, s.updated_at, src.key, src.environment_type, src.distro_name, \
                p.id, p.project_path, p.display_name, s.git_branch, s.message_count, \
                (SELECT COUNT(*) FROM sessions d WHERE d.external_session_id = s.external_session_id \
                   AND d.id <> s.id) \
         FROM sessions s \
         JOIN sources src ON src.id = s.source_id \
         LEFT JOIN projects p ON p.id = s.project_id \
         LEFT JOIN session_titles t ON t.external_session_id = s.external_session_id \
         WHERE 1 = 1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let marks = |n: usize| vec!["?"; n].join(",");
    if !p.source_keys.is_empty() {
        sql.push_str(&format!(" AND src.key IN ({})", marks(p.source_keys.len())));
        for k in &p.source_keys {
            args.push(Box::new(k.clone()));
        }
    }
    if !p.project_ids.is_empty() {
        sql.push_str(&format!(" AND s.project_id IN ({})", marks(p.project_ids.len())));
        for id in &p.project_ids {
            args.push(Box::new(*id));
        }
    }
    if !p.tag_ids.is_empty() {
        let count_sql = format!(
            "(SELECT COUNT(DISTINCT st.tag_id) FROM session_tags st \
               WHERE st.external_session_id = s.external_session_id AND st.tag_id IN ({}))",
            marks(p.tag_ids.len())
        );
        match p.tag_mode {
            TagMode::And => sql.push_str(&format!(" AND {count_sql} = {}", p.tag_ids.len())),
            TagMode::Or => sql.push_str(&format!(" AND {count_sql} > 0")),
        }
        for id in &p.tag_ids {
            args.push(Box::new(*id));
        }
    }
    if let Some(from) = p.updated_from.as_ref().filter(|t| !t.is_empty()) {
        sql.push_str(" AND s.updated_at >= ?");
        args.push(Box::new(from.clone()));
    }
    if let Some(to) = p.updated_to.as_ref().filter(|t| !t.is_empty()) {
        sql.push_str(" AND s.updated_at < ?");
        args.push(Box::new(to.clone()));
    }
    sql.push_str(" ORDER BY ");
    sql.push_str(&order_by(p.sort, p.sort_dir));

    let mut stmt = conn.prepare(&sql)?;
    let arg_refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
    let rows = stmt.query_map(arg_refs.as_slice(), |r| {
        Ok(SearchHit {
            session_id: r.get(0)?,
            external_session_id: r.get(1)?,
            title: r.get(2)?,
            renamed: r.get(3)?,
            started_at: r.get(4)?,
            updated_at: r.get(5)?,
            source_key: r.get(6)?,
            environment_type: r.get(7)?,
            distro_name: r.get(8)?,
            project_id: r.get(9)?,
            project_path: r.get(10)?,
            project_name: r.get(11)?,
            git_branch: r.get(12)?,
            message_count: r.get(13)?,
            duplicates: r.get(14)?,
            hit_count: 0,
            tags: Vec::new(),
            snippet: None,
            first_hit_message: None,
        })
    })?;

    let mut all = Vec::new();
    for row in rows {
        let mut hit = row?;
        if let Some(m) = &matched {
            let Some((mid, n)) = m.get(&hit.session_id) else {
                continue;
            };
            hit.hit_count = *n;
            hit.first_hit_message = *mid;
        }
        all.push(hit);
    }
    if p.sort == SortField::Hits && matched.is_some() {
        // Stable sort keeps the SQL order (newest first) among equal hit counts.
        match p.sort_dir {
            SortDir::Desc => all.sort_by(|a, b| b.hit_count.cmp(&a.hit_count)),
            SortDir::Asc => all.sort_by(|a, b| a.hit_count.cmp(&b.hit_count)),
        }
    }

    let total = all.len();
    all.truncate(limit);
    let tags = tags_by_session(conn)?;
    for hit in &mut all {
        if let Some(mid) = hit.first_hit_message {
            hit.snippet = message_text(conn, mid)?.map(|t| make_snippet(&t, &terms));
        }
        if let Some(ext) = &hit.external_session_id {
            // Duplicated session ids share tags; clone so each copy shows them.
            hit.tags = tags.get(ext).cloned().unwrap_or_default();
        }
    }
    Ok(SearchResult { hits: all, total })
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
    /// Title recorded by the agent, shown when the user resets a rename.
    pub original_title: String,
    pub renamed: bool,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub file_path: String,
    pub source_key: String,
    pub environment_type: String,
    pub distro_name: Option<String>,
    pub linux_home: Option<String>,
    pub bad_lines: i64,
    pub tags: Vec<TagView>,
    pub messages: Vec<MessageView>,
}

pub fn session_detail(conn: &Connection, id: i64) -> rusqlite::Result<Option<SessionDetail>> {
    let detail = conn
        .query_row(
            "SELECT s.id, s.external_session_id, coalesce(t.title, s.title), s.title, t.title IS NOT NULL, \
                    s.cwd, s.git_branch, s.started_at, s.updated_at, s.file_path, src.key, \
                    src.environment_type, src.distro_name, src.linux_home, s.bad_lines \
             FROM sessions s JOIN sources src ON src.id = s.source_id \
             LEFT JOIN session_titles t ON t.external_session_id = s.external_session_id \
             WHERE s.id = ?1",
            [id],
            |r| {
                Ok(SessionDetail {
                    session_id: r.get(0)?,
                    external_session_id: r.get(1)?,
                    title: r.get(2)?,
                    original_title: r.get(3)?,
                    renamed: r.get(4)?,
                    cwd: r.get(5)?,
                    git_branch: r.get(6)?,
                    started_at: r.get(7)?,
                    updated_at: r.get(8)?,
                    file_path: r.get(9)?,
                    source_key: r.get(10)?,
                    environment_type: r.get(11)?,
                    distro_name: r.get(12)?,
                    linux_home: r.get(13)?,
                    bad_lines: r.get(14)?,
                    tags: Vec::new(),
                    messages: Vec::new(),
                })
            },
        )
        .optional()?;
    let Some(mut detail) = detail else {
        return Ok(None);
    };
    if let Some(ext) = &detail.external_session_id {
        detail.tags = session_tags(conn, ext)?;
    }
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

fn session_tags(conn: &Connection, external_session_id: &str) -> rusqlite::Result<Vec<TagView>> {
    let mut stmt = conn.prepare(
        "SELECT g.id, g.name, g.color FROM session_tags st JOIN tags g ON g.id = st.tag_id \
         WHERE st.external_session_id = ?1 ORDER BY g.name COLLATE NOCASE",
    )?;
    stmt.query_map([external_session_id], |r| Ok(TagView { id: r.get(0)?, name: r.get(1)?, color: r.get(2)? }))?
        .collect()
}

/// Sets or clears (with `None` / blank) the user's display name for a session. History files
/// are not touched: the name exists only in this app.
pub fn rename_session(conn: &Connection, external_session_id: &str, title: Option<&str>) -> rusqlite::Result<()> {
    match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => conn.execute(
            "INSERT INTO session_titles(external_session_id, title) VALUES (?1, ?2) \
             ON CONFLICT(external_session_id) DO UPDATE SET title = excluded.title",
            params![external_session_id, t],
        )?,
        None => conn.execute("DELETE FROM session_titles WHERE external_session_id = ?1", [external_session_id])?,
    };
    Ok(())
}

/// After a title was written to the history file: show it as the agent's own title right away
/// (the next scan re-reads the file and finds the same value) and drop any app-only override.
pub fn apply_history_title(conn: &mut Connection, file_path: &str, external_session_id: &str, title: &str) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    let row: Option<(i64, Option<String>)> = tx
        .query_row("SELECT id, cwd FROM sessions WHERE file_path = ?1", [file_path], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    if let Some((id, cwd)) = row {
        tx.execute("UPDATE sessions SET title = ?1 WHERE id = ?2", params![title, id])?;
        tx.execute("DELETE FROM sessions_fts WHERE rowid = ?1", [id])?;
        tx.execute(
            "INSERT INTO sessions_fts(rowid, title, project_path) VALUES (?1, ?2, ?3)",
            params![id, title, cwd.unwrap_or_default()],
        )?;
    }
    tx.execute("DELETE FROM session_titles WHERE external_session_id = ?1", [external_session_id])?;
    tx.commit()
}

#[derive(Debug, Serialize)]
pub struct TagUsage {
    #[serde(flatten)]
    pub tag: TagView,
    pub session_count: i64,
}

pub fn list_tags(conn: &Connection) -> rusqlite::Result<Vec<TagUsage>> {
    let mut stmt = conn.prepare(
        "SELECT g.id, g.name, g.color, COUNT(st.external_session_id) FROM tags g \
         LEFT JOIN session_tags st ON st.tag_id = g.id \
         GROUP BY g.id ORDER BY g.name COLLATE NOCASE",
    )?;
    stmt.query_map([], |r| {
        Ok(TagUsage {
            tag: TagView { id: r.get(0)?, name: r.get(1)?, color: r.get(2)? },
            session_count: r.get(3)?,
        })
    })?
    .collect()
}

pub fn is_valid_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn validate_tag(name: &str, color: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::new("tag_name_empty"));
    }
    if name.chars().count() > 40 {
        return Err(AppError::new("tag_name_too_long"));
    }
    if !is_valid_color(color) {
        return Err(AppError::with("invalid_color", color));
    }
    Ok(name.to_string())
}

fn unique_violation(e: &rusqlite::Error) -> bool {
    matches!(e, rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation)
}

pub fn create_tag(conn: &Connection, name: &str, color: &str) -> Result<TagView, AppError> {
    let name = validate_tag(name, color)?;
    conn.execute("INSERT INTO tags(name, color) VALUES (?1, ?2)", params![name, color])
        .map_err(|e| if unique_violation(&e) { AppError::with("tag_exists", &name) } else { e.into() })?;
    Ok(TagView { id: conn.last_insert_rowid(), name, color: color.to_string() })
}

pub fn update_tag(conn: &Connection, id: i64, name: &str, color: &str) -> Result<TagView, AppError> {
    let name = validate_tag(name, color)?;
    let n = conn
        .execute("UPDATE tags SET name = ?1, color = ?2 WHERE id = ?3", params![name, color, id])
        .map_err(|e| if unique_violation(&e) { AppError::with("tag_exists", &name) } else { e.into() })?;
    if n == 0 {
        return Err(AppError::new("tag_not_found"));
    }
    Ok(TagView { id, name, color: color.to_string() })
}

pub fn delete_tag(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM tags WHERE id = ?1", [id])?;
    Ok(())
}

/// Replaces the tags of a session.
pub fn set_session_tags(conn: &mut Connection, external_session_id: &str, tag_ids: &[i64]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM session_tags WHERE external_session_id = ?1", [external_session_id])?;
    for id in tag_ids {
        tx.execute(
            "INSERT OR IGNORE INTO session_tags(external_session_id, tag_id) \
             SELECT ?1, id FROM tags WHERE id = ?2",
            params![external_session_id, id],
        )?;
    }
    tx.commit()
}

#[derive(Debug, Serialize)]
pub struct ProjectView {
    pub id: i64,
    pub source_key: String,
    pub project_path: String,
    pub display_name: String,
    pub session_count: i64,
    pub last_updated_at: Option<String>,
}

pub fn list_projects(conn: &Connection) -> rusqlite::Result<Vec<ProjectView>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, src.key, p.project_path, p.display_name, COUNT(s.id), MAX(s.updated_at) \
         FROM projects p JOIN sources src ON src.id = p.source_id \
         LEFT JOIN sessions s ON s.project_id = p.id \
         GROUP BY p.id ORDER BY MAX(s.updated_at) DESC",
    )?;
    stmt.query_map([], |r| {
        Ok(ProjectView {
            id: r.get(0)?,
            source_key: r.get(1)?,
            project_path: r.get(2)?,
            display_name: r.get(3)?,
            session_count: r.get(4)?,
            last_updated_at: r.get(5)?,
        })
    })?
    .collect()
}

#[derive(Debug, Serialize, Clone)]
pub struct ScanErrorView {
    pub file_path: Option<String>,
    pub code: String,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SourceStatus {
    pub key: String,
    pub root_path: String,
    pub last_scan_at: Option<String>,
    pub status: String,
    pub session_count: i64,
    pub recent_errors: Vec<ScanErrorView>,
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
        "SELECT file_path, code, detail FROM scan_errors WHERE source_id = ?1 ORDER BY id DESC LIMIT 5",
    )?;
    let mut out = HashMap::new();
    for (id, mut st) in rows {
        st.recent_errors = err_stmt
            .query_map([id], |r| Ok(ScanErrorView { file_path: r.get(0)?, code: r.get(1)?, detail: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?;
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

    #[test]
    fn tag_validation() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("t.sqlite")).unwrap();
        assert!(is_valid_color("#a1B2c3"));
        assert!(!is_valid_color("red"));
        let t = create_tag(&conn, " 顧客A ", "#ef4444").unwrap();
        assert_eq!(t.name, "顧客A");
        assert_eq!(create_tag(&conn, "顧客a", "#ef4444").unwrap_err().code, "tag_exists", "names are case-insensitive");
        assert_eq!(create_tag(&conn, "  ", "#ef4444").unwrap_err().code, "tag_name_empty");
        assert_eq!(create_tag(&conn, "x", "blue").unwrap_err().code, "invalid_color");
        let u = update_tag(&conn, t.id, "顧客B", "#22c55e").unwrap();
        assert_eq!(u.color, "#22c55e");
        assert_eq!(update_tag(&conn, 999, "y", "#000000").unwrap_err().code, "tag_not_found");
    }

    #[test]
    fn migrates_v1_project_tags() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.sqlite");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE project_tags (project_path TEXT PRIMARY KEY, client_tag TEXT NOT NULL); \
                 INSERT INTO project_tags VALUES ('C:\\a', '顧客A'), ('C:\\b', '顧客A'); \
                 CREATE TABLE sessions (id INTEGER PRIMARY KEY, old_column TEXT);",
            )
            .unwrap();
        }
        let conn = open(&path).unwrap();
        let tags = list_tags(&conn).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].tag.name, "顧客A");
        // Old index tables were rebuilt with the current schema.
        conn.prepare("SELECT external_session_id FROM sessions").unwrap();
    }
}
