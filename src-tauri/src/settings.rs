//! User settings, stored as JSON in the app's data directory (never synced or uploaded).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub windows_enabled: bool,
    /// WSL distributions the user allowed the app to read. Empty by default: reading a
    /// distribution starts it, so it is opt-in.
    pub enabled_wsl_distros: Vec<String>,
    /// Extra history roots (folders containing per-project folders of `*.jsonl`).
    pub extra_roots: Vec<String>,
    /// Folders or files whose sessions are never indexed.
    pub excluded_paths: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            windows_enabled: true,
            enabled_wsl_distros: Vec::new(),
            extra_roots: Vec::new(),
            excluded_paths: Vec::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("設定を保存できません: {e}"))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("設定を保存できません: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_defaults_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert!(Settings::load(&path).windows_enabled);
        std::fs::write(&path, r#"{"enabled_wsl_distros":["Ubuntu"]}"#).unwrap();
        let mut s = Settings::load(&path);
        assert!(s.windows_enabled);
        assert_eq!(s.enabled_wsl_distros, vec!["Ubuntu"]);
        s.excluded_paths.push(r"C:\secret".into());
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).excluded_paths, vec![r"C:\secret"]);
    }
}
