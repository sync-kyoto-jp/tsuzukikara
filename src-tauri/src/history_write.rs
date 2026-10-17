//! The one place that writes to agent history: appending a session title chosen by the user.
//!
//! Claude Code takes a session's title from the last `custom-title` record in its transcript,
//! so appending one record renames the session there as well. This only runs when the user
//! explicitly asks for it in the rename dialog. The record is appended with a single write in
//! append mode; existing content is never rewritten.

use crate::error::AppError;
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// A transcript modified more recently than this may belong to a running session.
const ACTIVE_WINDOW: Duration = Duration::from_secs(120);

/// Lines inspected when confirming that the file belongs to the expected session.
const ID_SCAN_LINES: usize = 200;

pub fn recently_active(file: &Path) -> bool {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < ACTIVE_WINDOW)
}

fn file_session_id(file: &Path) -> std::io::Result<Option<String>> {
    let reader = BufReader::new(std::fs::File::open(file)?);
    for line in reader.split(b'\n').take(ID_SCAN_LINES) {
        let line = line?;
        if let Ok(v) = serde_json::from_slice::<Value>(&line) {
            if let Some(id) = v.get("sessionId").and_then(Value::as_str) {
                return Ok(Some(id.to_string()));
            }
        }
    }
    Ok(None)
}

fn ends_with_newline(file: &mut std::fs::File) -> std::io::Result<bool> {
    let len = file.seek(SeekFrom::End(0))?;
    if len == 0 {
        return Ok(true);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

pub fn append_custom_title(file: &Path, session_id: &str, title: &str) -> Result<(), AppError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::new("title_empty"));
    }
    if !file.is_file() || file.extension().is_none_or(|e| e != "jsonl") {
        return Err(AppError::with("history_file_invalid", file.display()));
    }
    // Guard against writing a title into another session's file.
    match file_session_id(file) {
        Ok(Some(id)) if id == session_id => {}
        Ok(_) => return Err(AppError::with("history_session_mismatch", file.display())),
        Err(e) => return Err(AppError::with("history_write_failed", e)),
    }

    let record = serde_json::json!({
        "type": "custom-title",
        "customTitle": title,
        "sessionId": session_id,
    });
    let mut f = OpenOptions::new()
        .read(true)
        .append(true)
        .open(file)
        .map_err(|e| AppError::with("history_write_failed", e))?;
    let mut line = String::new();
    if !ends_with_newline(&mut f).map_err(|e| AppError::with("history_write_failed", e))? {
        line.push('\n');
    }
    line.push_str(&record.to_string());
    line.push('\n');
    // One write call so the record is not interleaved with a concurrent appender.
    f.write_all(line.as_bytes())
        .and_then(|_| f.flush())
        .map_err(|e| AppError::with("history_write_failed", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    const ID: &str = "dddddddd-0000-0000-0000-000000000004";

    fn transcript(dir: &Path, body: &str) -> std::path::PathBuf {
        let f = dir.join(format!("{ID}.jsonl"));
        std::fs::write(&f, body).unwrap();
        f
    }

    #[test]
    fn appends_title_that_the_parser_picks_up() {
        let dir = tempfile::tempdir().unwrap();
        let original = format!(
            "{{\"type\":\"user\",\"sessionId\":\"{ID}\",\"message\":{{\"role\":\"user\",\"content\":\"hi\"}}}}\n\
             {{\"type\":\"custom-title\",\"customTitle\":\"old\",\"sessionId\":\"{ID}\"}}\n"
        );
        let f = transcript(dir.path(), &original);
        append_custom_title(&f, ID, "  見積もり \"v2\"  ").unwrap();

        let content = std::fs::read_to_string(&f).unwrap();
        assert!(content.starts_with(&original), "existing content is untouched");
        let added = content[original.len()..].trim_end();
        let v: Value = serde_json::from_str(added).unwrap();
        assert_eq!(v["customTitle"], "見積もり \"v2\"");
        assert_eq!(parser::parse_reader(content.as_bytes()).title(), "見積もり \"v2\"");
    }

    #[test]
    fn adds_missing_trailing_newline_first() {
        let dir = tempfile::tempdir().unwrap();
        let f = transcript(dir.path(), &format!("{{\"type\":\"user\",\"sessionId\":\"{ID}\"}}"));
        append_custom_title(&f, ID, "new").unwrap();
        let content = std::fs::read_to_string(&f).unwrap();
        assert_eq!(content.lines().count(), 2);
        assert!(content.ends_with('\n'));
    }

    #[test]
    fn refuses_wrong_session_or_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = transcript(dir.path(), "{\"type\":\"user\",\"sessionId\":\"other\"}\n");
        assert_eq!(append_custom_title(&f, ID, "x").unwrap_err().code, "history_session_mismatch");
        assert_eq!(std::fs::read_to_string(&f).unwrap().lines().count(), 1, "nothing written");
        let txt = dir.path().join("a.txt");
        std::fs::write(&txt, "").unwrap();
        assert_eq!(append_custom_title(&txt, ID, "x").unwrap_err().code, "history_file_invalid");
        assert_eq!(append_custom_title(&f, ID, "  ").unwrap_err().code, "title_empty");
        assert!(recently_active(&f));
    }
}
