//! Which clients this server serves: its own protocol level and the one before.
//!
//! A client says which level it speaks (`X-Toybox-Api`; none means level 1) and, since the shared
//! version, who it is (`X-Toybox-Client: toybox/<version>` or `tbx/<version>`). A client below the
//! window gets HTTP 426 and a JSON error that carries the server version and the oldest client that is
//! still accepted. A client *newer* than the server is the client's business: it reads `apiVersion` and
//! `serverVersion` from `/info` and tells the user to update the server.

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::error::ApiError;
use crate::state::AppState;
use crate::{API_VERSION, SERVER_VERSION};

/// The first Toybox version that speaks each protocol level. A level a version speaks is the oldest
/// version that is still served once that level is the oldest in the window.
pub const PROTOCOL_SINCE: &[(u32, &str)] = &[(1, "0.1.0"), (2, "0.4.1")];

/// The oldest protocol level a server at `api_version` serves: its own and the one before.
pub fn oldest_level(api_version: u32) -> u32 {
    api_version.saturating_sub(1).max(1)
}

/// The oldest Toybox version that speaks `level` (or newer): what a refused client is told to update to.
pub fn oldest_client_version(level: u32) -> &'static str {
    PROTOCOL_SINCE.iter().rev().find(|(l, _)| *l <= level).map(|(_, v)| *v).unwrap_or(SERVER_VERSION)
}

/// The version a client names in `X-Toybox-Client`, without the product: `toybox/0.4.1` gives `0.4.1`.
pub fn client_version(header: &str) -> Option<semver::Version> {
    let (_, version) = header.trim().split_once('/')?;
    semver::Version::parse(version.trim()).ok()
}

/// Whether a request with this level and client header is inside the window.
pub fn accepts(level: u32, client_header: Option<&str>, oldest_level: u32) -> bool {
    if level < oldest_level {
        return false;
    }
    match client_header.and_then(client_version) {
        Some(version) => semver::Version::parse(oldest_client_version(oldest_level)).map(|oldest| version >= oldest).unwrap_or(true),
        None => true,
    }
}

/// The window, as a middleware after authentication: a client below it is told so.
pub async fn enforce(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let oldest = state.options.min_api_version.unwrap_or_else(|| oldest_level(API_VERSION));
    let level = request.headers().get("x-toybox-api").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(1);
    let client = request.headers().get("x-toybox-client").and_then(|v| v.to_str().ok()).map(str::to_string);
    if accepts(level, client.as_deref(), oldest) {
        return next.run(request).await;
    }
    let minimum = oldest_client_version(oldest);
    ApiError::new(
        StatusCode::UPGRADE_REQUIRED,
        "client_too_old",
        format!("This server is Toybox {SERVER_VERSION} and no longer accepts this version of the app. Update Toybox to {minimum} or newer."),
    )
    .with_extra(json!({
        "serverVersion": SERVER_VERSION,
        "apiVersion": API_VERSION,
        "minApiVersion": oldest,
        "minClientVersion": minimum,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_the_servers_level_and_the_one_before() {
        assert_eq!(oldest_level(2), 1);
        assert_eq!(oldest_level(3), 2);
        assert_eq!(oldest_level(1), 1);
        assert!(accepts(1, None, oldest_level(2)));
        assert!(accepts(2, Some("toybox/0.4.1"), oldest_level(2)));
        assert!(!accepts(1, None, oldest_level(3)));
        assert!(accepts(2, Some("tbx/0.5.0"), oldest_level(3)));
    }

    #[test]
    fn a_client_names_its_version_after_a_slash() {
        assert_eq!(client_version("toybox/0.4.1").unwrap().to_string(), "0.4.1");
        assert_eq!(client_version("tbx/1.2.3-rc.1").unwrap().to_string(), "1.2.3-rc.1");
        assert!(client_version("toybox").is_none());
        assert!(client_version("toybox/x").is_none());
    }

    #[test]
    fn a_stated_version_older_than_the_window_is_refused_even_at_the_right_level() {
        assert!(!accepts(2, Some("toybox/0.3.0"), 2));
        assert!(accepts(2, Some("toybox/0.4.1"), 2));
        assert!(accepts(2, Some("garbage"), 2));
    }

    #[test]
    fn the_oldest_version_for_a_level_comes_from_the_table() {
        assert_eq!(oldest_client_version(1), "0.1.0");
        assert_eq!(oldest_client_version(2), "0.4.1");
        assert_eq!(oldest_client_version(9), "0.4.1");
    }
}
