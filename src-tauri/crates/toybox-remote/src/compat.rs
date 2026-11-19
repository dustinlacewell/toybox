//! Do this Toybox and a library's server fit together, and what to tell the person.
//!
//! The server serves clients of its own protocol level and the one before (see the server's `compat`);
//! a client below that gets HTTP 426, a client above the server's level reads "server too old" here.
//! Between those, differing versions are only a notice: everything works.
//! The sentences are written here once and shown by the app and by `tbx`.

use semver::Version;
use toybox_core::infra::library::RemoteCompat;

use crate::client::API_LEVEL;
use crate::error::RemoteError;
use crate::types::Info;

/// The version of this Toybox: the app and `tbx` are released with the server under one version.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const OK: &str = "ok";
pub const NOTICE: &str = "notice";
pub const SERVER_TOO_OLD: &str = "server_too_old";
pub const CLIENT_TOO_OLD: &str = "client_too_old";

fn parse(text: &str) -> Option<Version> {
    Version::parse(text.trim().trim_start_matches('v')).ok()
}

/// Compare what `/info` says with this program (`app_version`, normally [`APP_VERSION`]).
pub fn assess(info: &Info, app_version: &str) -> RemoteCompat {
    let mut compat = RemoteCompat {
        state: OK.into(),
        server_version: info.server_version.clone(),
        min_client_version: info.min_client_version.clone(),
        min_api_version: info.min_api_version,
        message: None,
    };
    let server = if info.server_version.is_empty() { "an older version".to_string() } else { info.server_version.clone() };
    // This Toybox speaks a newer protocol than the server.
    if info.api_version < API_LEVEL {
        compat.state = SERVER_TOO_OLD.into();
        compat.message = Some(RemoteError::ServerTooOld { found: info.api_version, needed: API_LEVEL, server_version: info.server_version.clone() }.to_string());
        return compat;
    }
    // The server stopped serving a Toybox as old as this one (it would have answered 426 already).
    let below_level = info.min_api_version.is_some_and(|min| min > API_LEVEL);
    let below_version = match (info.min_client_version.as_deref().and_then(parse), parse(app_version)) {
        (Some(min), Some(mine)) => mine < min,
        _ => false,
    };
    if below_level || below_version {
        compat.state = CLIENT_TOO_OLD.into();
        compat.message = Some(
            RemoteError::ClientTooOld {
                server_version: info.server_version.clone(),
                min_client_version: info.min_client_version.clone().unwrap_or_else(|| info.server_version.clone()),
            }
            .to_string(),
        );
        return compat;
    }
    if let (Some(theirs), Some(mine)) = (parse(&info.server_version), parse(app_version)) {
        if theirs < mine {
            compat.state = NOTICE.into();
            compat.message = Some(format!(
                "This server runs Toybox {server}; this is Toybox {app_version}. They work together. Ask whoever runs the server to run \"toybox-server update\" so they match."
            ));
        } else if theirs > mine {
            compat.state = NOTICE.into();
            compat.message = Some(format!("This server runs Toybox {server}; this is Toybox {app_version}. They work together. Update Toybox to keep up with the server."));
        }
    }
    compat
}

/// What a failed request says about the fit, when it says anything.
pub fn from_error(error: &RemoteError) -> Option<RemoteCompat> {
    match error {
        RemoteError::ServerTooOld { server_version, .. } => Some(RemoteCompat { state: SERVER_TOO_OLD.into(), server_version: server_version.clone(), min_client_version: None, min_api_version: None, message: Some(error.to_string()) }),
        RemoteError::ClientTooOld { server_version, min_client_version } => Some(RemoteCompat {
            state: CLIENT_TOO_OLD.into(),
            server_version: server_version.clone(),
            min_client_version: Some(min_client_version.clone()).filter(|v| !v.is_empty()),
            min_api_version: None,
            message: Some(error.to_string()),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toybox_core::infra::library::RemoteRole;

    fn info(server: &str, api: u32, min_api: Option<u32>, min_client: Option<&str>) -> Info {
        Info {
            name: "S".into(),
            server_version: server.into(),
            api_version: api,
            min_api_version: min_api,
            min_client_version: min_client.map(str::to_string),
            role: RemoteRole::Read,
            revision: 1,
            db_id: String::new(),
            thumb_revision: 0,
        }
    }

    #[test]
    fn matching_versions_say_nothing() {
        let compat = assess(&info("0.5.0", 2, Some(1), Some("0.1.0")), "0.5.0");
        assert_eq!((compat.state.as_str(), compat.message), ("ok", None));
    }

    #[test]
    fn an_older_or_newer_server_inside_the_window_is_a_notice() {
        let older = assess(&info("0.4.1", 2, Some(1), Some("0.1.0")), "0.5.0");
        assert_eq!(older.state, NOTICE);
        let text = older.message.unwrap();
        assert!(text.contains("0.4.1") && text.contains("0.5.0") && text.contains("toybox-server update"), "{text}");
        let newer = assess(&info("0.6.0", 2, Some(1), Some("0.1.0")), "0.5.0");
        assert_eq!(newer.state, NOTICE);
        assert!(newer.message.unwrap().contains("Update Toybox"));
    }

    #[test]
    fn a_server_below_this_protocol_is_too_old() {
        let compat = assess(&info("0.3.0", 1, None, None), "0.5.0");
        assert_eq!(compat.state, SERVER_TOO_OLD);
        let text = compat.message.unwrap();
        assert!(text.contains("0.3.0") && text.contains("toybox-server update"), "{text}");
    }

    #[test]
    fn a_server_that_wants_a_newer_toybox_is_reported_as_such() {
        let compat = assess(&info("0.7.0", 3, Some(3), Some("0.6.0")), "0.5.0");
        assert_eq!(compat.state, CLIENT_TOO_OLD);
        let text = compat.message.unwrap();
        assert!(text.contains("0.7.0") && text.contains("Update Toybox to 0.6.0"), "{text}");
        // By the version alone, too.
        assert_eq!(assess(&info("0.7.0", 2, Some(2), Some("0.6.0")), "0.5.0").state, CLIENT_TOO_OLD);
    }

    #[test]
    fn a_426_becomes_a_client_too_old_sentence() {
        let body = br#"{"error":{"code":"client_too_old","message":"x"},"serverVersion":"0.7.0","minClientVersion":"0.6.0"}"#;
        let error = crate::client::status_error(426, None, body, "s");
        assert_eq!(error, RemoteError::ClientTooOld { server_version: "0.7.0".into(), min_client_version: "0.6.0".into() });
        assert_eq!(error.code(), "client_too_old");
        assert!(from_error(&error).unwrap().message.unwrap().contains("Update Toybox to 0.6.0"));
    }
}
