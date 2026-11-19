//! The GUI-free host layer shared by the desktop app and `tbx`. See docs/cli.md.
//!
//! A [`Host`] owns what the app used to keep in Tauri state: where the per-user
//! data lives ([`HostPaths`]), the registry of libraries, the remote-library hub,
//! the programs plugins started and the background fingerprinting. Operations are
//! synchronous and blocking methods on it, grouped by module:
//!
//! * [`paths`], [`host`]: construction.
//! * [`libraries`], [`remote`]: the registered libraries; resolving a library id to
//!   a local or remote handle; remote servers and their tokens.
//! * [`browse`]: queries over all readable libraries together and small personal writes.
//! * [`ops`]: everything that works on the items of one library (import, metadata,
//!   delete, transfer, export, thumbnails, scan, paths, previews).
//! * [`settings`]: app settings (saved exports, the converter path, plugin switches).
//! * [`plugins`], [`plugin_fs`], [`plugin_services`], [`bundled`]: plugin discovery and
//!   dependency rules, the jailed filesystem, plugin settings/tools/scratch, seeding.
//! * [`fingerprint`]: the background content-fingerprint pass.
//!
//! Only things that are inherently a window's (the asset-protocol scope, window
//! events, `plugin://`, dialogs) stay in the app; they get callbacks or are done
//! by the app after the call.

pub mod browse;
pub mod cli_install;
pub mod bundled;
pub mod fingerprint;
pub mod host;
pub mod edit;
pub mod inspect;
pub mod libraries;
pub mod lookup;
pub mod ops;
pub mod paths;
pub mod plugin_admin;
pub mod plugin_fs;
pub mod plugin_services;
pub mod plugins;
pub mod remote;
pub mod search;
pub mod secrets;
pub mod settings;
pub mod types;
pub mod visibility;

pub use host::Host;
pub use paths::HostPaths;
pub use toybox_core::error::{AppError, AppResult};
