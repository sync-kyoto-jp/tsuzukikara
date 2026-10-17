//! User-facing errors as stable codes. The UI translates `code` (see `src/i18n`), so the backend
//! never hard-codes display language. `detail` carries untranslated context such as a path or an
//! OS error message.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AppError {
    pub code: String,
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: &str) -> Self {
        Self { code: code.to_string(), detail: None }
    }

    pub fn with(code: &str, detail: impl std::fmt::Display) -> Self {
        Self { code: code.to_string(), detail: Some(detail.to_string()) }
    }

    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::with("internal", e)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            Some(d) => write!(f, "{}: {d}", self.code),
            None => f.write_str(&self.code),
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::internal(e)
    }
}
