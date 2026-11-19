//! Everything that can go wrong talking to a server, in plain language. The
//! text of an error is meant to be shown to the user as it is, so it never
//! contains the access token, and it names the server by its address only.

use std::fmt;

use toybox_core::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteError {
    /// No connection: the address is wrong, the server is down, there is no
    /// network, or it did not answer in time.
    Unreachable { server: String, detail: String },
    /// The secure connection could not be set up (certificate problems and the like).
    Tls { server: String, detail: String },
    /// The server did not accept the token (wrong, mistyped or revoked).
    BadToken,
    /// The token is valid but its role is too low for this.
    Forbidden(String),
    /// Too many wrong tokens from this address.
    LockedOut { retry_after: Option<u64> },
    NotFound(String),
    /// The server refused because of the state of things (last admin, name taken, import busy).
    Conflict { code: String, message: String },
    /// Any other refusal, with the server's own explanation.
    Rejected { status: u16, code: String, message: String },
    /// The server failed (HTTP 5xx).
    Server { status: u16, message: String },
    /// The answer was not what a Toybox server sends.
    Protocol(String),
    /// The server is a Toybox server, but older than this app can follow.
    ServerTooOld { found: u32, needed: u32, server_version: String },
    /// The server no longer accepts a Toybox as old as this one (HTTP 426, or its `/info` says so).
    ClientTooOld { server_version: String, min_client_version: String },
    /// A problem on this computer (disk, credential store).
    Local(String),
}

pub type RemoteResult<T> = Result<T, RemoteError>;

impl RemoteError {
    /// A short stable name, for code and tests.
    pub fn code(&self) -> &str {
        match self {
            RemoteError::Unreachable { .. } => "unreachable",
            RemoteError::Tls { .. } => "tls",
            RemoteError::BadToken => "unauthorized",
            RemoteError::Forbidden(_) => "forbidden",
            RemoteError::LockedOut { .. } => "locked_out",
            RemoteError::NotFound(_) => "not_found",
            RemoteError::Conflict { code, .. } | RemoteError::Rejected { code, .. } => code,
            RemoteError::Server { .. } => "server_error",
            RemoteError::Protocol(_) => "protocol",
            RemoteError::ServerTooOld { .. } => "server_too_old",
            RemoteError::ClientTooOld { .. } => "client_too_old",
            RemoteError::Local(_) => "local",
        }
    }

    /// The server could not be reached at all (as opposed to answering "no"): the
    /// library falls back to its cached catalog and shows as offline.
    pub fn is_connectivity(&self) -> bool {
        matches!(self, RemoteError::Unreachable { .. } | RemoteError::Tls { .. })
    }

    /// The token is not accepted or not allowed.
    pub fn is_access(&self) -> bool {
        matches!(self, RemoteError::BadToken | RemoteError::Forbidden(_) | RemoteError::LockedOut { .. })
    }
}

fn wait_phrase(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds} seconds"),
        60..=119 => "1 minute".to_string(),
        s => format!("{} minutes", s.div_ceil(60)),
    }
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoteError::Unreachable { server, detail } => write!(
                f,
                "Cannot reach the server at {server} ({detail}). Check your connection and the address, or try again later."
            ),
            RemoteError::Tls { server, detail } => write!(
                f,
                "Could not make a secure connection to {server} ({detail}). The server's certificate may be missing, expired or for a different name."
            ),
            RemoteError::BadToken => f.write_str(
                "The server did not accept this access token. It may be mistyped or may have been revoked. Use \"Change token…\" to enter a new one.",
            ),
            RemoteError::Forbidden(message) => f.write_str(message),
            RemoteError::LockedOut { retry_after } => match retry_after {
                Some(seconds) => write!(
                    f,
                    "The server is refusing this computer for a while because wrong access tokens were sent too often. Try again in {}.",
                    wait_phrase(*seconds)
                ),
                None => f.write_str("The server is refusing this computer for a while because wrong access tokens were sent too often. Try again later."),
            },
            RemoteError::NotFound(message) => f.write_str(message),
            RemoteError::Conflict { message, .. } => f.write_str(message),
            RemoteError::Rejected { message, .. } => f.write_str(message),
            RemoteError::Server { message, .. } => write!(f, "The server reported a problem: {message}"),
            RemoteError::Protocol(detail) => write!(
                f,
                "The server sent an answer Toybox does not understand ({detail}). Is this address really a Toybox server, and is it up to date?"
            ),
            RemoteError::ServerTooOld { found, needed, server_version } => {
                if server_version.is_empty() {
                    write!(f, "This server is too old for this version of Toybox: it speaks version {found} of the library protocol and this app needs version {needed}. Update the server, or use an older Toybox with it.")
                } else {
                    write!(
                        f,
                        "This server runs Toybox {server_version}, which is too old for this Toybox ({}): it speaks version {found} of the library protocol and this needs version {needed}. Ask whoever runs it to update the server (toybox-server update), or use an older Toybox with it.",
                        crate::compat::APP_VERSION
                    )
                }
            }
            RemoteError::ClientTooOld { server_version, min_client_version } => write!(
                f,
                "This server runs Toybox {server_version} and no longer accepts this Toybox ({}). Update Toybox to {min_client_version} or newer.",
                crate::compat::APP_VERSION
            ),
            RemoteError::Local(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for RemoteError {}

impl From<RemoteError> for AppError {
    fn from(error: RemoteError) -> Self {
        AppError::msg(error.to_string())
    }
}

impl From<AppError> for RemoteError {
    fn from(error: AppError) -> Self {
        RemoteError::Local(error.to_string())
    }
}

impl From<std::io::Error> for RemoteError {
    fn from(error: std::io::Error) -> Self {
        RemoteError::Local(format!("A file on this computer could not be read or written: {error}"))
    }
}

impl From<crate::secrets::SecretError> for RemoteError {
    fn from(error: crate::secrets::SecretError) -> Self {
        RemoteError::Local(error.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_sentences_and_never_mention_a_token() {
        let errors = [
            RemoteError::Unreachable { server: "https://a.example.com".into(), detail: "connection refused".into() },
            RemoteError::Tls { server: "https://a.example.com".into(), detail: "certificate expired".into() },
            RemoteError::BadToken,
            RemoteError::LockedOut { retry_after: Some(900) },
            RemoteError::LockedOut { retry_after: None },
            RemoteError::Protocol("not JSON".into()),
        ];
        for error in errors {
            let text = error.to_string();
            assert!(text.ends_with('.') || text.ends_with('?'), "{text}");
            assert!(!text.contains("tbx_"), "{text}");
        }
        assert!(RemoteError::LockedOut { retry_after: Some(900) }.to_string().contains("15 minutes"));
        assert!(RemoteError::Unreachable { server: String::new(), detail: String::new() }.is_connectivity());
        assert!(RemoteError::BadToken.is_access() && !RemoteError::BadToken.is_connectivity());
    }
}
