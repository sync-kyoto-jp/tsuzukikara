//! Optional, opt-in check for a newer release on GitHub.
//!
//! This is the app's only network access. It runs only after the user turned it on (or pressed
//! "Check now"), at most once a day, and sends a single unauthenticated GET for the latest
//! release of this repository. No history, settings or identifiers are sent. The WebView keeps
//! its no-network CSP; the request is made here in the backend.

use crate::error::AppError;
use serde::Serialize;
use std::time::Duration;

pub const REPO: &str = "sync-kyoto-jp/tsuzukikara";
const TIMEOUT: Duration = Duration::from_secs(10);
/// Automatic checks are skipped if the last one was more recent than this.
pub const CHECK_INTERVAL_HOURS: i64 = 24;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

fn parse_version(s: &str) -> Option<semver::Version> {
    semver::Version::parse(s.trim().trim_start_matches('v')).ok()
}

/// True when `latest` is a strictly newer version than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Only links to this repository's release pages are ever opened.
pub fn is_release_url(url: &str) -> bool {
    url.starts_with(&format!("https://github.com/{REPO}/releases/"))
        && url.chars().all(|c| c.is_ascii_alphanumeric() || "/:._-".contains(c))
}

fn parse_release(body: &str) -> Result<Release, AppError> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| AppError::with("update_check_failed", e))?;
    let tag = v["tag_name"].as_str().unwrap_or_default();
    let url = v["html_url"].as_str().unwrap_or_default();
    let version = parse_version(tag).ok_or_else(|| AppError::with("update_check_failed", format!("tag {tag}")))?;
    if !is_release_url(url) {
        return Err(AppError::with("invalid_release_url", url));
    }
    Ok(Release { version: version.to_string(), url: url.to_string() })
}

/// Fetches the latest published release. `Ok(None)` when the repository has no release yet.
pub fn fetch_latest(current_version: &str) -> Result<Option<Release>, AppError> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    let response = agent
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", &format!("Tsuzukikara/{current_version}"))
        .call();
    match response {
        Ok(r) => {
            let body = r.into_string().map_err(|e| AppError::with("update_check_failed", e))?;
            parse_release(&body).map(Some)
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(e) => Err(AppError::with("update_check_failed", e)),
    }
}

/// Opens a release page in the default browser.
pub fn open_release_page(url: &str) -> Result<(), AppError> {
    if !is_release_url(url) {
        return Err(AppError::with("invalid_release_url", url));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url])
            .creation_flags(0x0800_0000)
            .spawn()
            .map(|_| ())
            .map_err(|e| AppError::with("open_url_failed", e))
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| AppError::with("open_url_failed", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.2.0"));
        assert!(!is_newer("nightly", "0.1.0"));
    }

    #[test]
    fn validates_release_urls() {
        assert!(is_release_url("https://github.com/sync-kyoto-jp/tsuzukikara/releases/tag/v0.2.0"));
        assert!(!is_release_url("https://github.com/other/repo/releases/tag/v1.0.0"));
        assert!(!is_release_url("https://github.com/sync-kyoto-jp/tsuzukikara/releases/tag/v1&calc"));
        assert!(!is_release_url("http://github.com/sync-kyoto-jp/tsuzukikara/releases/"));
    }

    /// Calls the real GitHub API. Run with: cargo test -- --ignored fetches_latest_release
    #[test]
    #[ignore]
    fn fetches_latest_release() {
        let latest = fetch_latest("0.0.0").expect("request failed");
        println!("latest release: {latest:?}");
    }

    #[test]
    fn parses_github_release_json() {
        let body = r#"{"tag_name":"v0.3.1","html_url":"https://github.com/sync-kyoto-jp/tsuzukikara/releases/tag/v0.3.1","draft":false}"#;
        assert_eq!(
            parse_release(body).unwrap(),
            Release {
                version: "0.3.1".into(),
                url: "https://github.com/sync-kyoto-jp/tsuzukikara/releases/tag/v0.3.1".into()
            }
        );
        let evil = r#"{"tag_name":"v9.9.9","html_url":"https://evil.example/releases/x"}"#;
        assert_eq!(parse_release(evil).unwrap_err().code, "invalid_release_url");
    }
}
