//! What can go wrong, as the exit status and the one-line message of docs/cli.md.

use std::fmt;

use toybox_core::error::AppError;
use toybox_host::lookup::LookupError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The command failed.
    Failed,
    /// Wrong usage.
    Usage,
    /// Something named does not exist (a library, an item, a plugin).
    NotFound,
    /// The command finished but some items failed or were skipped with an error.
    Partial,
    /// The person said no (or was never asked and nothing was done).
    Cancelled,
}

impl Kind {
    pub fn code(self) -> &'static str {
        match self {
            Kind::Failed => "failed",
            Kind::Usage => "usage",
            Kind::NotFound => "not_found",
            Kind::Partial => "partial",
            Kind::Cancelled => "cancelled",
        }
    }

    pub fn exit_status(self) -> i32 {
        match self {
            Kind::Failed | Kind::Cancelled => 1,
            Kind::Usage => 2,
            Kind::NotFound => 3,
            Kind::Partial => 4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CliError {
    pub kind: Kind,
    pub message: String,
    pub hint: Option<String>,
}

pub type CliResult<T> = Result<T, CliError>;

impl CliError {
    fn new(kind: Kind, message: impl Into<String>) -> Self {
        CliError { kind, message: message.into(), hint: None }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(Kind::Failed, message)
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(Kind::Usage, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(Kind::NotFound, message)
    }

    pub fn partial(message: impl Into<String>) -> Self {
        Self::new(Kind::Partial, message)
    }

    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(Kind::Cancelled, message)
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

const NO_LIBRARY_HINT: &str = "register one with `tbx library create <DIR>` or `tbx library add <DIR>`";

impl From<AppError> for CliError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::NoLibrary => CliError::failed("no library is registered").hint(NO_LIBRARY_HINT),
            other => CliError::failed(other.to_string()),
        }
    }
}

impl From<LookupError> for CliError {
    fn from(error: LookupError) -> Self {
        let message = error.to_string();
        match error {
            LookupError::NoLibraries => CliError::not_found(message).hint(NO_LIBRARY_HINT),
            LookupError::NoDefault => CliError::usage(message).hint("name one with -L, or choose one with `tbx library default <LIBRARY>`"),
            LookupError::NotFound(_) => CliError::not_found(message).hint("see `tbx library list`"),
            LookupError::Ambiguous(..) => CliError::usage(message).hint("use the id instead of the name"),
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(error: std::io::Error) -> Self {
        CliError::failed(error.to_string())
    }
}

impl From<serde_json::Error> for CliError {
    fn from(error: serde_json::Error) -> Self {
        CliError::failed(error.to_string())
    }
}
