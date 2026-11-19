//! Remote libraries for the Toybox desktop app: a blocking client for
//! `toybox-server` (see docs/server.md), the local file cache a remote library
//! is read from, shared-thumbnail sync, uploads, and transfers between any two
//! libraries. No GUI dependency, and no async runtime: `toybox-core` is
//! synchronous, so this is too.
//!
//! * [`url`]      what the user types as a server address
//! * [`secrets`]  tokens in the OS credential store (a trait, so tests use memory)
//! * [`client`]   one method per server route, errors in plain language
//! * [`files`]    the on-demand file cache
//! * [`library`]  a remote library: the replica kept up to date from the server's
//!                 revision feed, offline fallback, thumbnails, delete
//! * [`upload`]   importing staged files and uploading transfer packages
//! * [`transfer`] copy / move between local and remote libraries
//! * [`hub`]      the open remote libraries of the app

pub mod client;
pub mod compat;
pub mod error;
pub mod files;
pub mod hub;
pub mod library;
pub mod progress;
pub mod secrets;
pub mod transfer;
pub mod types;
pub mod upload;
pub mod url;

pub use error::{RemoteError, RemoteResult};
pub use hub::Hub;
pub use library::RemoteLibrary;

use toybox_core::infra::library::RemoteRole;

/// What a server says about itself and the token it was given.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    /// The normalised address.
    pub url: String,
    /// Shown when the address is not encrypted.
    pub warning: Option<String>,
    pub server_name: String,
    pub role: RemoteRole,
    /// A sentence about the versions of the server and this Toybox, when they differ.
    pub notice: Option<String>,
    /// The version comparison behind the notice (kept with the library once it is added).
    pub compat: toybox_core::infra::library::RemoteCompat,
}

/// Check an address and token without saving anything: normalise the address,
/// ask the server who it is and which role the token has.
pub fn probe(address: &str, token: &str) -> RemoteResult<Probe> {
    let server = url::normalize_server_url(address).map_err(RemoteError::Local)?;
    let token = token.trim();
    if token.is_empty() {
        return Err(RemoteError::Local("Enter the access token the server's owner gave you.".into()));
    }
    let info = client::Client::new(&server.url, token).info()?;
    if info.api_version < client::API_LEVEL {
        return Err(RemoteError::ServerTooOld { found: info.api_version, needed: client::API_LEVEL, server_version: info.server_version });
    }
    let compat = compat::assess(&info, compat::APP_VERSION);
    if compat.state == compat::CLIENT_TOO_OLD {
        return Err(RemoteError::ClientTooOld { server_version: info.server_version, min_client_version: info.min_client_version.unwrap_or_default() });
    }
    Ok(Probe { url: server.url, warning: server.warning, server_name: info.name, role: info.role, notice: compat.message.clone(), compat })
}
