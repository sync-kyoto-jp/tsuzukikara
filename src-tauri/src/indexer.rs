//! Incremental, read-only import of session files into the index.
//!
//! A file is re-parsed only when its size or modification time changed since the last scan.
//! Files are opened read-only and never modified. Errors are recorded per file (path and a short
//! summary only — never transcript content) and do not stop the rest of the scan.

use crate::error::AppError;
use crate::parser::{self, ParsedSession};
use crate::sources::{self, SourceRoot};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Default, Serialize, Clone)]
pub struct ScanReport {
    pub source_key: String,
    pub status: String,
    pub files: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub errors: usize,
    /// Set when the whole source could not be scanned (e.g. a WSL distribution failed to start).
    pub error: Option<AppError>,
}

/// Hashes bytes as they are read, so the file is read once for both parsing and hashing.
struct HashingReader<R> {
    inner: R,
    hash: u64,
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        for b in &buf[..n] {
            // FNV-1a: only used to tell file revisions apart, not for security.
            self.hash ^= u64::from(*b);
            self.hash = self.hash.wrapping_mul(0x0100_0000_01b3);
        }
        Ok(n)
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn upsert_source(conn: &Connection, root: &SourceRoot) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO sources(key, agent_type, environment_type, distro_name, linux_home, root_path) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(key) DO UPDATE SET root_path = excluded.root_path, \
             linux_home = excluded.linux_home, distro_name = excluded.distro_name",
        params![
            root.key,
            root.agent_type,
            root.environment_type.as_str(),
            root.distro_name,
            root.linux_home,
            root.root_path.to_string_lossy(),
        ],
    )?;
    conn.query_row("SELECT id FROM sources WHERE key = ?1", [&root.key], |r| r.get(0))
}

/// Records a per-file problem as a code (translated by the UI) plus untranslated detail.
fn record_error(conn: &Connection, source_id: i64, file: Option<&Path>, code: &str, detail: Option<String>) {
    // Best effort: failing to record an error must not abort the scan.
    let _ = conn.execute(
        "INSERT INTO scan_errors(source_id, occurred_at, file_path, code, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![source_id, now_rfc3339(), file.map(|f| f.to_string_lossy().into_owned()), code, detail],
    );
}

fn is_excluded(path: &Path, excludes: &[PathBuf]) -> bool {
    let p = path.to_string_lossy().to_lowercase().replace('/', "\\");
    excludes.iter().any(|e| {
        let e = e.to_string_lossy().to_lowercase().replace('/', "\\");
        let e = e.trim_end_matches('\\');
        !e.is_empty() && (p == e || p.starts_with(&format!("{e}\\")))
    })
}

fn display_name(project_path: &str) -> String {
    project_path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(project_path)
        .to_string()
}

fn upsert_project(tx: &Transaction, source_id: i64, project_path: &str) -> rusqlite::Result<i64> {
    tx.execute(
        "INSERT INTO projects(source_id, project_path, display_name) VALUES (?1, ?2, ?3) \
         ON CONFLICT(source_id, project_path) DO NOTHING",
        params![source_id, project_path, display_name(project_path)],
    )?;
    tx.query_row(
        "SELECT id FROM projects WHERE source_id = ?1 AND project_path = ?2",
        params![source_id, project_path],
        |r| r.get(0),
    )
}

fn store_session(
    conn: &mut Connection,
    source_id: i64,
    file: &Path,
    size: i64,
    mtime: i64,
    hash: u64,
    parsed: &ParsedSession,
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    let file_str = file.to_string_lossy();
    tx.execute("DELETE FROM sessions WHERE file_path = ?1", [&file_str])?;

    // Claude Code names the project folder after the cwd; use it if no record carried a cwd.
    let project_path = parsed.cwd.clone().unwrap_or_else(|| {
        file.parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let project_id = upsert_project(&tx, source_id, &project_path)?;
    let external_id = parsed.session_id.clone().or_else(|| {
        file.file_stem().map(|s| s.to_string_lossy().into_owned())
    });

    tx.execute(
        "INSERT INTO sessions(source_id, project_id, external_session_id, title, cwd, git_branch, \
             started_at, updated_at, file_path, file_size, file_mtime, content_hash, message_count, bad_lines) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            source_id,
            project_id,
            external_id,
            parsed.title(),
            parsed.cwd,
            parsed.git_branch,
            parsed.started_at,
            parsed.updated_at,
            file_str,
            size,
            mtime,
            format!("{hash:016x}"),
            parsed.messages.len() as i64,
            parsed.bad_lines as i64,
        ],
    )?;
    let session_id = tx.last_insert_rowid();
    {
        let mut stmt = tx.prepare(
            "INSERT INTO messages(session_id, sequence_no, role, timestamp, searchable_text) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for (i, m) in parsed.messages.iter().enumerate() {
            stmt.execute(params![session_id, i as i64, m.role, m.timestamp, m.text])?;
        }
    }
    tx.commit()
}

/// Scans one source root. `progress(done, total)` is called after each file.
pub fn scan_source(
    conn: &mut Connection,
    root: &SourceRoot,
    excludes: &[PathBuf],
    progress: &mut dyn FnMut(usize, usize),
) -> rusqlite::Result<ScanReport> {
    let source_id = upsert_source(conn, root)?;
    conn.execute("DELETE FROM scan_errors WHERE source_id = ?1", [source_id])?;
    let mut report = ScanReport { source_key: root.key.clone(), ..Default::default() };

    let files = match sources::session_files(&root.root_path) {
        Ok(f) => f,
        Err(e) => {
            report.status = if e.kind() == std::io::ErrorKind::NotFound { "not_found" } else { "error" }.into();
            record_error(conn, source_id, Some(&root.root_path), "root_unreadable", Some(e.to_string()));
            report.errors = 1;
            conn.execute(
                "UPDATE sources SET status = ?1, last_scan_at = ?2 WHERE id = ?3",
                params![report.status, now_rfc3339(), source_id],
            )?;
            return Ok(report);
        }
    };

    let files: Vec<PathBuf> = files.into_iter().filter(|f| !is_excluded(f, excludes)).collect();
    report.files = files.len();
    let mut seen: HashSet<String> = HashSet::new();

    for (i, file) in files.iter().enumerate() {
        let file_str = file.to_string_lossy().into_owned();
        seen.insert(file_str.clone());
        let meta = match std::fs::metadata(file) {
            Ok(m) => m,
            Err(e) => {
                record_error(conn, source_id, Some(file), "file_stat_failed", Some(e.to_string()));
                report.errors += 1;
                progress(i + 1, files.len());
                continue;
            }
        };
        let size = meta.len() as i64;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let existing: Option<(i64, i64, i64)> = conn
            .query_row(
                "SELECT file_size, file_mtime, bad_lines FROM sessions WHERE file_path = ?1",
                [&file_str],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((s, m, bad)) = existing {
            if s == size && m == mtime {
                report.unchanged += 1;
                if bad > 0 {
                    record_error(conn, source_id, Some(file), "lines_skipped", Some(bad.to_string()));
                }
                progress(i + 1, files.len());
                continue;
            }
        }

        let parsed = std::fs::File::open(file).map(|f| {
            let mut reader = HashingReader { inner: f, hash: 0xcbf2_9ce4_8422_2325 };
            let parsed = parser::parse_reader(&mut reader);
            (parsed, reader.hash)
        });
        match parsed {
            Ok((parsed, hash)) => {
                if parsed.bad_lines > 0 {
                    record_error(conn, source_id, Some(file), "lines_skipped", Some(parsed.bad_lines.to_string()));
                }
                store_session(conn, source_id, file, size, mtime, hash, &parsed)?;
                report.updated += 1;
            }
            Err(e) => {
                record_error(conn, source_id, Some(file), "file_open_failed", Some(e.to_string()));
                report.errors += 1;
            }
        }
        progress(i + 1, files.len());
    }

    // Drop sessions whose files disappeared or are now excluded.
    let indexed: Vec<String> = {
        let mut stmt = conn.prepare("SELECT file_path FROM sessions WHERE source_id = ?1")?;
        stmt.query_map([source_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    for path in indexed.into_iter().filter(|p| !seen.contains(p)) {
        conn.execute("DELETE FROM sessions WHERE file_path = ?1", [&path])?;
        report.removed += 1;
    }
    conn.execute(
        "DELETE FROM projects WHERE source_id = ?1 AND id NOT IN \
         (SELECT DISTINCT project_id FROM sessions WHERE project_id IS NOT NULL)",
        [source_id],
    )?;

    // Every per-file problem (unreadable file, skipped lines) was recorded in scan_errors.
    report.errors = conn.query_row(
        "SELECT COUNT(*) FROM scan_errors WHERE source_id = ?1",
        [source_id],
        |r| r.get::<_, i64>(0),
    )? as usize;
    report.status = "ok".into();
    conn.execute(
        "UPDATE sources SET status = ?1, last_scan_at = ?2 WHERE id = ?3",
        params![report.status, now_rfc3339(), source_id],
    )?;
    Ok(report)
}

/// Removes a source that is no longer configured, together with its indexed sessions.
pub fn remove_source(conn: &Connection, key: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM sources WHERE key = ?1", [key])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::sources::custom_root;

    fn write_session(dir: &Path, project: &str, id: &str, lines: &[String]) -> PathBuf {
        let p = dir.join(project);
        std::fs::create_dir_all(&p).unwrap();
        let f = p.join(format!("{id}.jsonl"));
        std::fs::write(&f, lines.join("\n")).unwrap();
        f
    }

    fn user(id: &str, ts: &str, cwd: &str, text: &str) -> String {
        serde_json::json!({
            "type": "user", "sessionId": id, "timestamp": ts, "cwd": cwd,
            "message": {"role": "user", "content": text}
        })
        .to_string()
    }

    fn assistant(id: &str, ts: &str, text: &str) -> String {
        serde_json::json!({
            "type": "assistant", "sessionId": id, "timestamp": ts,
            "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}
        })
        .to_string()
    }

    fn search(conn: &Connection, q: &str) -> db::SearchResult {
        db::search(conn, &db::SearchParams { query: q.into(), ..Default::default() }).unwrap()
    }

    #[test]
    fn indexes_searches_and_rescans_incrementally() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().join("projects");
        let a = "aaaaaaaa-0000-0000-0000-000000000001";
        let b = "bbbbbbbb-0000-0000-0000-000000000002";
        write_session(&root_dir, "C--work-alpha", a, &[
            user(a, "2026-03-01T10:00:00.000Z", r"C:\work\alpha", "請求書の自動生成バッチを直したい"),
            assistant(a, "2026-03-01T10:01:00.000Z", "Invoice batch を確認します"),
        ]);
        let fb = write_session(&root_dir, "C--work-beta", b, &[
            user(b, "2026-03-05T09:00:00.000Z", r"C:\work\beta", "ログイン画面のUI改善"),
            "{broken json".to_string(),
        ]);

        let mut conn = db::open(&tmp.path().join("index.sqlite")).unwrap();
        let root = custom_root(&root_dir);
        let r = scan_source(&mut conn, &root, &[], &mut |_, _| {}).unwrap();
        assert_eq!((r.files, r.updated, r.unchanged), (2, 2, 0));
        assert_eq!(r.errors, 1, "broken line is reported");

        // Japanese substring (trigram), English case-insensitive, short-term LIKE fallback.
        let res = search(&conn, "自動生成");
        assert_eq!(res.total, 1);
        assert_eq!(res.hits[0].external_session_id.as_deref(), Some(a));
        assert!(res.hits[0].snippet.as_ref().unwrap().contains("\u{E000}自動生成\u{E001}"));
        assert_eq!(search(&conn, "invoice").total, 1);
        assert_eq!(search(&conn, "UI").total, 1);
        // Every term must match somewhere in the same session.
        assert_eq!(search(&conn, "請求書 ログイン").total, 0);
        // Project path matches too.
        assert_eq!(search(&conn, "alpha").total, 1);
        // Empty query lists everything, newest first.
        let all = search(&conn, "");
        assert_eq!(all.total, 2);
        assert_eq!(all.hits[0].external_session_id.as_deref(), Some(b));
        assert_eq!(all.hits[0].project_name.as_deref(), Some("beta"));

        // Date filter.
        let res = db::search(&conn, &db::SearchParams {
            updated_from: Some("2026-03-03T00:00:00Z".into()),
            ..Default::default()
        }).unwrap();
        assert_eq!(res.total, 1);

        // Unchanged files are skipped; removed files are dropped.
        let r = scan_source(&mut conn, &root, &[], &mut |_, _| {}).unwrap();
        assert_eq!((r.updated, r.unchanged), (0, 2));
        std::fs::remove_file(&fb).unwrap();
        let r = scan_source(&mut conn, &root, &[], &mut |_, _| {}).unwrap();
        assert_eq!(r.removed, 1);
        assert_eq!(search(&conn, "ログイン").total, 0, "FTS rows are removed with the session");

        // Exclusions remove already-indexed sessions.
        let r = scan_source(&mut conn, &root, &[root_dir.join("C--work-alpha")], &mut |_, _| {}).unwrap();
        assert_eq!((r.files, r.removed), (0, 1));
        assert_eq!(search(&conn, "").total, 0);
    }

    #[test]
    fn detail_and_tags() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().join("projects");
        let a = "cccccccc-0000-0000-0000-000000000003";
        write_session(&root_dir, "C--work-gamma", a, &[
            user(a, "2026-04-01T10:00:00.000Z", r"C:\work\gamma", "質問です"),
            assistant(a, "2026-04-01T10:00:05.000Z", "回答です"),
        ]);
        let mut conn = db::open(&tmp.path().join("index.sqlite")).unwrap();
        scan_source(&mut conn, &custom_root(&root_dir), &[], &mut |_, _| {}).unwrap();

        let hit = &search(&conn, "").hits[0];
        let d = db::session_detail(&conn, hit.session_id).unwrap().unwrap();
        assert_eq!(d.messages.len(), 2);
        assert_eq!(d.messages[1].role, "assistant");
        assert_eq!(d.title, "質問です");

        // Multiple coloured tags; filtering requires every selected tag; tag names are searchable.
        let g = db::create_tag(&conn, "顧客G", "#ef4444").unwrap();
        let urgent = db::create_tag(&conn, "urgent", "#f59e0b").unwrap();
        db::set_session_tags(&mut conn, a, &[g.id, urgent.id]).unwrap();
        let hit = &search(&conn, "").hits[0];
        assert_eq!(hit.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["urgent", "顧客G"]);
        fn by_tags(conn: &Connection, ids: Vec<i64>) -> usize {
            db::search(conn, &db::SearchParams { tag_ids: ids, ..Default::default() }).unwrap().total
        }
        assert_eq!(by_tags(&conn, vec![g.id, urgent.id]), 1);
        db::set_session_tags(&mut conn, a, &[g.id]).unwrap();
        assert_eq!(by_tags(&conn, vec![g.id, urgent.id]), 0);
        assert_eq!(search(&conn, "顧客G").total, 1);

        // Renaming changes only the display title and is searchable; clearing restores it.
        db::rename_session(&conn, a, Some("  見積もり対応  ")).unwrap();
        let d = db::session_detail(&conn, hit.session_id).unwrap().unwrap();
        assert_eq!((d.title.as_str(), d.original_title.as_str(), d.renamed), ("見積もり対応", "質問です", true));
        assert_eq!(search(&conn, "見積").total, 1);

        // User data survives index deletion and a rescan.
        db::clear_index(&conn).unwrap();
        assert_eq!(search(&conn, "").total, 0);
        scan_source(&mut conn, &custom_root(&root_dir), &[], &mut |_, _| {}).unwrap();
        let hit = &search(&conn, "").hits[0];
        assert_eq!(hit.title, "見積もり対応");
        assert_eq!(hit.tags.len(), 1);

        db::rename_session(&conn, a, Some("")).unwrap();
        assert_eq!(search(&conn, "").hits[0].title, "質問です");
        db::delete_tag(&conn, g.id).unwrap();
        assert!(search(&conn, "").hits[0].tags.is_empty());
    }

    #[test]
    fn title_written_to_history_survives_rescan() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().join("projects");
        let a = "eeeeeeee-0000-0000-0000-000000000005";
        let file = write_session(&root_dir, "C--work-delta", a, &[
            user(a, "2026-05-01T10:00:00.000Z", r"C:\work\delta", "最初の質問"),
        ]);
        let mut conn = db::open(&tmp.path().join("index.sqlite")).unwrap();
        let root = custom_root(&root_dir);
        scan_source(&mut conn, &root, &[], &mut |_, _| {}).unwrap();
        db::rename_session(&conn, a, Some("仮の名前")).unwrap();

        let path = file.to_string_lossy().into_owned();
        crate::history_write::append_custom_title(&file, a, "正式名称").unwrap();
        db::apply_history_title(&mut conn, &path, a, "正式名称").unwrap();
        let hit = &search(&conn, "正式名称").hits[0];
        assert_eq!((hit.title.as_str(), hit.renamed), ("正式名称", false), "app-only override is dropped");

        let r = scan_source(&mut conn, &root, &[], &mut |_, _| {}).unwrap();
        assert_eq!(r.updated, 1, "the appended file is re-read");
        let hit = &search(&conn, "").hits[0];
        assert_eq!((hit.title.as_str(), hit.renamed), ("正式名称", false));
    }

    /// Measures a full scan and a few searches against the local Windows history. Prints only
    /// counts and timings, never content. Run with: cargo test --release -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_local_history() {
        let Some(root) = crate::sources::windows_root() else { return };
        let tmp = tempfile::tempdir().unwrap();
        let mut conn = db::open(&tmp.path().join("index.sqlite")).unwrap();
        let t = std::time::Instant::now();
        let r = scan_source(&mut conn, &root, &[], &mut |_, _| {}).unwrap();
        println!("full scan: {:?} files={} updated={} errors={}", t.elapsed(), r.files, r.updated, r.errors);
        let t = std::time::Instant::now();
        let r = scan_source(&mut conn, &root, &[], &mut |_, _| {}).unwrap();
        println!("rescan:    {:?} unchanged={}", t.elapsed(), r.unchanged);
        let msgs: i64 = conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0)).unwrap();
        let size = std::fs::metadata(tmp.path().join("index.sqlite")).unwrap().len();
        println!("messages={msgs} db_bytes={size}");
        for q in ["", "エラー", "要件定義", "test", "UI", "API 認証"] {
            let t = std::time::Instant::now();
            let res = search(&conn, q);
            println!("search {q:?}: {:?} total={}", t.elapsed(), res.total);
        }
    }

    #[test]
    fn exclusion_matching() {
        let ex = vec![PathBuf::from(r"C:\Users\x\.claude\projects\C--secret")];
        assert!(is_excluded(Path::new(r"c:\users\x\.claude\projects\C--secret\a.jsonl"), &ex));
        assert!(!is_excluded(Path::new(r"C:\Users\x\.claude\projects\C--secret2\a.jsonl"), &ex));
        assert_eq!(display_name("/home/u/repo/"), "repo");
        assert_eq!(display_name(r"C:\work\alpha"), "alpha");
    }
}
