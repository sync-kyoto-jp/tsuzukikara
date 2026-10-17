//! Read-only parser for Claude Code session transcripts (`~/.claude/projects/<dir>/<session>.jsonl`).
//!
//! The JSONL format is undocumented and changes between Claude Code versions, so the parser is
//! deliberately lenient: unknown record types are ignored and malformed lines are counted, never
//! fatal. Tool results are NOT indexed: they are the most likely place for secrets (env dumps,
//! tokens, file contents) and they dominate file size.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read};

/// Longest label derived from the first prompt when a session has no title.
const LABEL_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedMessage {
    pub role: String,
    pub timestamp: Option<String>,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct ParsedSession {
    pub session_id: Option<String>,
    pub custom_title: Option<String>,
    pub ai_title: Option<String>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub messages: Vec<ParsedMessage>,
    /// Lines that were not valid JSON (e.g. a partially written last line).
    pub bad_lines: usize,
}

impl ParsedSession {
    /// Display title: user-set title, then the agent's generated title, then the first prompt.
    pub fn title(&self) -> String {
        if let Some(t) = self.custom_title.as_deref().or(self.ai_title.as_deref()) {
            if !t.trim().is_empty() {
                return t.trim().to_string();
            }
        }
        self.messages
            .iter()
            .find(|m| m.role == "user")
            .map(|m| label_from(&m.text))
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "(無題のセッション)".to_string())
    }
}

fn label_from(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut label: String = one_line.chars().take(LABEL_CHARS).collect();
    if one_line.chars().count() > LABEL_CHARS {
        label.push('…');
    }
    label
}

pub fn parse_reader<R: Read>(reader: R) -> ParsedSession {
    let mut out = ParsedSession::default();
    for line in BufReader::new(reader).split(b'\n') {
        let Ok(line) = line else {
            out.bad_lines += 1;
            break;
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<Value>(&line) {
            Ok(v) => apply_record(&mut out, &v),
            Err(_) => out.bad_lines += 1,
        }
    }
    out
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn apply_record(out: &mut ParsedSession, v: &Value) {
    if out.session_id.is_none() {
        out.session_id = str_field(v, "sessionId").map(str::to_string);
    }
    if let Some(ts) = str_field(v, "timestamp") {
        if out.started_at.as_deref().is_none_or(|s| ts < s) {
            out.started_at = Some(ts.to_string());
        }
        if out.updated_at.as_deref().is_none_or(|s| ts > s) {
            out.updated_at = Some(ts.to_string());
        }
    }
    if out.cwd.is_none() {
        out.cwd = str_field(v, "cwd").map(str::to_string);
    }
    if let Some(b) = str_field(v, "gitBranch").filter(|b| !b.is_empty()) {
        out.git_branch = Some(b.to_string());
    }

    match str_field(v, "type") {
        Some("custom-title") => {
            out.custom_title = str_field(v, "customTitle").map(str::to_string);
        }
        Some("ai-title") => {
            out.ai_title = str_field(v, "aiTitle")
                .or_else(|| str_field(v, "title"))
                .map(str::to_string);
        }
        Some(role @ ("user" | "assistant")) => {
            // Sub-agent turns and injected meta messages are not part of the visible conversation.
            if v.get("isSidechain").and_then(Value::as_bool) == Some(true)
                || v.get("isMeta").and_then(Value::as_bool) == Some(true)
            {
                return;
            }
            let Some(content) = v.get("message").and_then(|m| m.get("content")) else {
                return;
            };
            let text = extract_text(content);
            if text.trim().is_empty() || is_injected(&text) {
                return;
            }
            out.messages.push(ParsedMessage {
                role: role.to_string(),
                timestamp: str_field(v, "timestamp").map(str::to_string),
                text,
            });
        }
        _ => {}
    }
}

/// Text the user and assistant actually wrote. Thinking blocks and tool results are skipped;
/// tool calls are reduced to their tool name so the preview still shows what happened.
fn extract_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => {
            let mut parts = Vec::new();
            for b in blocks {
                match str_field(b, "type") {
                    Some("text") => {
                        if let Some(t) = str_field(b, "text") {
                            parts.push(t.to_string());
                        }
                    }
                    Some("tool_use") => {
                        if let Some(name) = str_field(b, "name") {
                            parts.push(format!("[tool: {name}]"));
                        }
                    }
                    _ => {}
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

/// Harness-generated user turns (slash-command plumbing, interrupts) that the user did not type.
fn is_injected(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<local-command-")
        || t.starts_with("<command-name>")
        || t.starts_with("<system-reminder>")
        || t.starts_with("[Request interrupted")
        || t.starts_with("Caveat: The messages below were generated")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-01-01T00:00:00.000Z","sessionId":"11111111-2222-3333-4444-555555555555"}
{"parentUuid":null,"isSidechain":false,"type":"user","message":{"role":"user","content":"ダミー案件の検索機能を作って"},"uuid":"a","timestamp":"2026-01-01T00:00:01.000Z","cwd":"C:\\work\\dummy","sessionId":"11111111-2222-3333-4444-555555555555","gitBranch":"main"}
{"type":"assistant","isSidechain":false,"message":{"role":"assistant","content":[{"type":"thinking","thinking":"secret reasoning"},{"type":"text","text":"了解しました。"},{"type":"tool_use","name":"Read","input":{}}]},"timestamp":"2026-01-01T00:00:02.000Z","sessionId":"11111111-2222-3333-4444-555555555555"}
{"type":"user","isSidechain":false,"message":{"role":"user","content":[{"type":"tool_result","content":"API_KEY=should-not-be-indexed"}]},"timestamp":"2026-01-01T00:00:03.000Z"}
{"type":"user","isSidechain":true,"message":{"role":"user","content":"subagent prompt"},"timestamp":"2026-01-01T00:00:04.000Z"}
{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"},"timestamp":"2026-01-01T00:00:04.500Z"}
this line is broken
{"type":"ai-title","aiTitle":"検索機能の実装","sessionId":"11111111-2222-3333-4444-555555555555"}
"#;

    #[test]
    fn parses_conversation_and_skips_noise() {
        let s = parse_reader(SAMPLE.as_bytes());
        assert_eq!(s.session_id.as_deref(), Some("11111111-2222-3333-4444-555555555555"));
        assert_eq!(s.cwd.as_deref(), Some("C:\\work\\dummy"));
        assert_eq!(s.git_branch.as_deref(), Some("main"));
        assert_eq!(s.started_at.as_deref(), Some("2026-01-01T00:00:00.000Z"));
        assert_eq!(s.updated_at.as_deref(), Some("2026-01-01T00:00:04.500Z"));
        assert_eq!(s.bad_lines, 1);
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.messages[1].text, "了解しました。\n[tool: Read]");
        let all: String = s.messages.iter().map(|m| m.text.as_str()).collect();
        assert!(!all.contains("API_KEY"), "tool results must not be indexed");
        assert!(!all.contains("secret reasoning"), "thinking must not be indexed");
        assert_eq!(s.title(), "検索機能の実装");
    }

    #[test]
    fn custom_title_wins_and_label_fallback() {
        let mut s = parse_reader(SAMPLE.as_bytes());
        s.custom_title = Some("顧客A 改修".into());
        assert_eq!(s.title(), "顧客A 改修");
        s.custom_title = None;
        s.ai_title = None;
        assert_eq!(s.title(), "ダミー案件の検索機能を作って");
    }

    #[test]
    fn empty_input_is_untitled() {
        let s = parse_reader(&b""[..]);
        assert!(s.messages.is_empty());
        assert_eq!(s.title(), "(無題のセッション)");
    }
}
