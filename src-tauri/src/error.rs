//! The single error type crossing the command boundary into JS.

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// No library configured yet. Serialized as a stable sentinel so the
    /// frontend can route this to the picker instead of the error screen.
    #[error("no library configured")]
    NoLibrary,
    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn msg(s: impl Into<String>) -> Self {
        AppError::Other(s.into())
    }
}

/// Serialize errors as their string message so JS receives a plain string.
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
