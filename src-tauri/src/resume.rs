//! Resuming a session in a new terminal with `claude --resume <session-id>`.
//!
//! Resume is offered only when the session id is well-formed, the working directory still exists
//! and the `claude` CLI is found in the session's environment. Every value placed on a command
//! line is validated first; the UI asks for confirmation before `launch` is called.

use crate::db::SessionDetail;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Serialize)]
pub struct ResumePlan {
    pub available: bool,
    /// Why resume is unavailable, in user-facing Japanese.
    pub reason: Option<String>,
    /// What will run, shown in the confirmation dialog.
    pub command_display: String,
    pub working_dir: Option<String>,
}

enum Target {
    Windows { cwd: String },
    Wsl { distro: String, cwd: String },
}

fn hidden(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

pub fn is_valid_session_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Rejects characters that could break out of the quoting used in `launch`.
fn is_safe_path(p: &str) -> bool {
    !p.is_empty() && !p.chars().any(|c| c.is_control() || matches!(c, '"' | '\'' | '`' | '$' | '\0'))
}

fn target_of(d: &SessionDetail) -> Result<Target, String> {
    let cwd = d.cwd.clone().ok_or("作業ディレクトリが履歴に記録されていません")?;
    if !is_safe_path(&cwd) {
        return Err("作業ディレクトリに再開コマンドで扱えない文字が含まれています".into());
    }
    match d.environment_type.as_str() {
        "wsl" => {
            let distro = d.distro_name.clone().ok_or("ディストリビューションが不明です")?;
            if !crate::sources::is_valid_distro_name(&distro) {
                return Err("不正なディストリビューション名です".into());
            }
            Ok(Target::Wsl { distro, cwd })
        }
        // Sessions found in a custom root were recorded by a Windows-side CLI as far as we know.
        _ => Ok(Target::Windows { cwd }),
    }
}

fn wsl_unc(distro: &str, linux_path: &str) -> PathBuf {
    let mut p = PathBuf::from(format!(r"\\wsl.localhost\{distro}"));
    for part in linux_path.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p
}

fn windows_cli_found() -> bool {
    hidden("where.exe").arg("claude").output().is_ok_and(|o| o.status.success())
}

const CLI_PATH_MARKER: &str = "CLAUDE_CLI_PATH=";

/// Absolute path of the real `claude` executable in `distro`, resolved from the home directory.
///
/// Version managers (nodenv, asdf, nvm) put a shim on PATH that picks a Node version per
/// directory, so `claude` may exist from `~` but not inside a project pinned to another Node
/// version. Resolving the shim to the real executable makes resume independent of that.
fn wsl_cli_path(distro: &str) -> Option<String> {
    // A login + interactive shell is needed for PATH set up in .bashrc/.profile. Its MOTD may
    // also print, so the answer is tagged with a marker. `-e` skips wsl's own shell re-parsing.
    let script = format!(
        "cd ~ || exit 1; p=$(command -v claude) || exit 1; \
         case \"$p\" in */shims/*) \
           r=$(nodenv which claude 2>/dev/null || asdf which claude 2>/dev/null) && p=$r ;; esac; \
         printf '\\n{CLI_PATH_MARKER}%s\\n' \"$(readlink -f \"$p\")\""
    );
    let out = hidden("wsl.exe")
        .args(["-d", distro, "-e", "bash", "-lic", &script])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix(CLI_PATH_MARKER).map(str::to_string))
        .filter(|p| p.starts_with('/') && is_safe_path(p) && !p.contains(char::is_whitespace))
}

/// Confirms the resolved executable actually starts inside the session's working directory.
fn wsl_cli_runs_in(distro: &str, cwd: &str, cli: &str) -> bool {
    hidden("wsl.exe")
        .args(["-d", distro, "--cd", cwd, "-e", cli, "--version"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Shell command run in the new WSL window. On failure the window stays open so the error can
/// be read instead of the console closing immediately.
fn wsl_resume_script(cli: &str, id: &str) -> String {
    format!(
        "'{cli}' --resume {id} || {{ s=$?; echo; echo claude exited with status $s; \
         read -r -p 'Press Enter to close' _; }}"
    )
}

pub fn plan(d: &SessionDetail) -> ResumePlan {
    let unavailable = |reason: String, display: String, cwd: Option<String>| ResumePlan {
        available: false,
        reason: Some(reason),
        command_display: display,
        working_dir: cwd,
    };
    let Some(id) = d.external_session_id.clone().filter(|i| is_valid_session_id(i)) else {
        return unavailable("セッションIDが不明または不正です".into(), String::new(), d.cwd.clone());
    };
    let display = format!("claude --resume {id}");
    let target = match target_of(d) {
        Ok(t) => t,
        Err(e) => return unavailable(e, display, d.cwd.clone()),
    };
    match target {
        Target::Windows { cwd } => {
            if !Path::new(&cwd).is_dir() {
                return unavailable(format!("作業ディレクトリが見つかりません: {cwd}"), display, Some(cwd));
            }
            if !windows_cli_found() {
                return unavailable("Windows 側で claude コマンドが見つかりません".into(), display, Some(cwd));
            }
            ResumePlan { available: true, reason: None, command_display: display, working_dir: Some(cwd) }
        }
        Target::Wsl { distro, cwd } => {
            let base = format!("wsl.exe -d {distro} --cd {cwd} -- {display}");
            if !wsl_unc(&distro, &cwd).is_dir() {
                return unavailable(format!("{distro} 内に作業ディレクトリが見つかりません: {cwd}"), base, Some(cwd));
            }
            let Some(cli) = wsl_cli_path(&distro) else {
                return unavailable(format!("{distro} 内で claude コマンドが見つかりません"), base, Some(cwd));
            };
            let display = format!("wsl.exe -d {distro} --cd {cwd} -- {cli} --resume {id}");
            if !wsl_cli_runs_in(&distro, &cwd, &cli) {
                return unavailable(
                    format!("{distro} の作業ディレクトリで claude を起動できません ({cli})"),
                    display,
                    Some(cwd),
                );
            }
            ResumePlan { available: true, reason: None, command_display: display, working_dir: Some(cwd) }
        }
    }
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Opens a new console window running `claude --resume`. Re-validates everything `plan` checked.
pub fn launch(d: &SessionDetail) -> Result<(), String> {
    let p = plan(d);
    if !p.available {
        return Err(p.reason.unwrap_or_else(|| "再開できません".into()));
    }
    let id = d.external_session_id.clone().unwrap_or_default();
    // PowerShell's Start-Process opens a visible window and, unlike cmd.exe, accepts UNC working
    // directories (Windows-side sessions opened on \\wsl.localhost\... paths).
    let script = match target_of(d)? {
        Target::Windows { cwd } => format!(
            "Start-Process -FilePath 'powershell.exe' -WorkingDirectory {} -ArgumentList '-NoExit','-Command','claude --resume {id}'",
            ps_quote(&cwd)
        ),
        Target::Wsl { distro, cwd } => {
            // Validated by plan(); resolved again here so a stale plan is never trusted.
            let cli = wsl_cli_path(&distro).ok_or("claude コマンドが見つかりません")?;
            // `-e` runs bash directly, so the script is not re-parsed by wsl's default shell.
            format!(
                "Start-Process -FilePath 'wsl.exe' -ArgumentList {}",
                ps_quote(&format!(
                    "-d {distro} --cd \"{cwd}\" -e bash -lic \"{}\"",
                    wsl_resume_script(&cli, &id)
                ))
            )
        }
    };
    hidden("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("ターミナルを起動できません: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_inputs() {
        assert!(is_valid_session_id("e454715f-8b30-435d-863e-4c3922156ab9"));
        assert!(!is_valid_session_id("abc; rm -rf /"));
        assert!(is_safe_path(r"C:\work\案件 A"));
        assert!(is_safe_path(r"\\wsl.localhost\Ubuntu\home\u\repo"));
        assert!(!is_safe_path("/home/u/$(whoami)"));
        assert!(!is_safe_path("C:\\a'b"));
        assert_eq!(ps_quote("a'b"), "'a''b'");
        assert_eq!(wsl_unc("Ubuntu", "/home/u/repo"), PathBuf::from(r"\\wsl.localhost\Ubuntu\home\u\repo"));
    }

    #[test]
    fn wsl_script_has_no_double_quotes() {
        // The script is wrapped in double quotes on the wsl.exe command line.
        let s = wsl_resume_script("/opt/claude/bin/claude", "e454715f-8b30-435d-863e-4c3922156ab9");
        assert!(!s.contains('"'));
        assert!(s.starts_with("'/opt/claude/bin/claude' --resume e454715f-"));
    }

    /// Checks CLI resolution against a real distribution. Run with:
    /// WSL_TEST_DISTRO=Ubuntu WSL_TEST_CWD=/path cargo test -- --ignored resolves_wsl_cli
    #[test]
    #[ignore]
    fn resolves_wsl_cli() {
        let distro = std::env::var("WSL_TEST_DISTRO").unwrap_or_else(|_| "Ubuntu".into());
        let cli = wsl_cli_path(&distro).expect("claude not resolved");
        println!("resolved: {cli}");
        assert!(!cli.contains("/shims/"));
        if let Ok(cwd) = std::env::var("WSL_TEST_CWD") {
            assert!(wsl_cli_runs_in(&distro, &cwd, &cli), "claude does not start in {cwd}");
        }
    }
}
