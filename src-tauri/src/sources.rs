//! Discovery of Claude Code history locations on Windows and in WSL distributions.
//!
//! Nothing here assumes fixed paths: the Windows root comes from `CLAUDE_CONFIG_DIR` or the user
//! profile, and a WSL root is resolved by asking the distribution for its `$HOME` — which starts
//! the distribution, so it only happens for distributions the user has explicitly enabled.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentType {
    Windows,
    Wsl,
    Custom,
}

impl EnvironmentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EnvironmentType::Windows => "windows",
            EnvironmentType::Wsl => "wsl",
            EnvironmentType::Custom => "custom",
        }
    }
}

/// A history root to scan: a directory whose children are per-project folders of `*.jsonl` files.
#[derive(Debug, Clone, Serialize)]
pub struct SourceRoot {
    /// Stable key, e.g. `windows`, `wsl:Ubuntu`, `custom:D:\backup\projects`.
    pub key: String,
    pub agent_type: String,
    pub environment_type: EnvironmentType,
    pub distro_name: Option<String>,
    /// Linux home of the WSL user, used to translate paths when resuming.
    pub linux_home: Option<String>,
    pub root_path: PathBuf,
}

pub fn windows_root() -> Option<SourceRoot> {
    let base = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".claude")))?;
    Some(SourceRoot {
        key: "windows".into(),
        agent_type: "claude_code".into(),
        environment_type: EnvironmentType::Windows,
        distro_name: None,
        linux_home: None,
        root_path: base.join("projects"),
    })
}

pub fn custom_root(path: &Path) -> SourceRoot {
    SourceRoot {
        key: format!("custom:{}", path.display()),
        agent_type: "claude_code".into(),
        environment_type: EnvironmentType::Custom,
        distro_name: None,
        linux_home: None,
        root_path: path.to_path_buf(),
    }
}

/// Distribution names are passed to `wsl.exe` and embedded in UNC paths, so only allow the
/// characters WSL itself permits.
pub fn is_valid_distro_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn wsl_command() -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new("wsl.exe");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// `wsl.exe` writes UTF-16LE on stdout for its own messages; fall back to UTF-8 for others.
fn decode_wsl_output(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes.len() % 2 == 0 && bytes.iter().skip(1).step_by(2).all(|b| *b == 0) {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Lists installed distributions without starting any of them.
pub fn list_wsl_distros() -> Result<Vec<String>, String> {
    let out = wsl_command()
        .args(["--list", "--quiet"])
        .output()
        .map_err(|e| format!("wsl.exe を実行できません: {e}"))?;
    if !out.status.success() {
        return Err("WSL のディストリビューション一覧を取得できませんでした".into());
    }
    Ok(decode_wsl_output(&out.stdout)
        .lines()
        .map(|l| l.trim().trim_matches('\u{feff}').to_string())
        .filter(|l| is_valid_distro_name(l))
        .collect())
}

/// Resolves the default user's home inside `distro` (starts the distribution).
pub fn resolve_wsl_root(distro: &str) -> Result<SourceRoot, String> {
    if !is_valid_distro_name(distro) {
        return Err("不正なディストリビューション名です".into());
    }
    let out = wsl_command()
        .args(["-d", distro, "--", "sh", "-c", "printf %s \"$HOME\""])
        .output()
        .map_err(|e| format!("wsl.exe を実行できません: {e}"))?;
    let home = decode_wsl_output(&out.stdout).trim().to_string();
    if !out.status.success() || !home.starts_with('/') {
        return Err(format!("{distro} のホームディレクトリを取得できませんでした"));
    }
    let mut root = PathBuf::from(format!(r"\\wsl.localhost\{distro}"));
    for part in home.split('/').filter(|p| !p.is_empty()) {
        root.push(part);
    }
    root.push(".claude");
    root.push("projects");
    Ok(SourceRoot {
        key: format!("wsl:{distro}"),
        agent_type: "claude_code".into(),
        environment_type: EnvironmentType::Wsl,
        distro_name: Some(distro.to_string()),
        linux_home: Some(home),
        root_path: root,
    })
}

/// Session transcripts directly inside each project folder. Sub-folders hold sub-agent
/// transcripts and spilled tool results, which are not sessions of their own.
pub fn session_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for project in std::fs::read_dir(root)? {
        let project = project?;
        if !project.file_type()?.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "jsonl")
                && entry.file_type().is_ok_and(|t| t.is_file())
            {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16_wsl_output() {
        let s: Vec<u8> = "Ubuntu\r\nDebian\r\n"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(decode_wsl_output(&s), "Ubuntu\r\nDebian\r\n");
        assert_eq!(decode_wsl_output(b"Ubuntu\n"), "Ubuntu\n");
    }

    #[test]
    fn validates_distro_names() {
        assert!(is_valid_distro_name("Ubuntu-22.04"));
        assert!(!is_valid_distro_name("Ubuntu; rm -rf"));
        assert!(!is_valid_distro_name(""));
    }

    #[test]
    fn finds_only_top_level_session_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("C--work-dummy");
        std::fs::create_dir_all(p.join("abc").join("subagents")).unwrap();
        std::fs::write(p.join("abc.jsonl"), "{}").unwrap();
        std::fs::write(p.join("abc").join("subagents").join("agent.jsonl"), "{}").unwrap();
        std::fs::write(p.join("notes.txt"), "x").unwrap();
        let files = session_files(dir.path()).unwrap();
        assert_eq!(files, vec![p.join("abc.jsonl")]);
    }
}
