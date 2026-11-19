//! Library registry state and folder-level operations that need no GUI host:
//! the in-memory `Registry`, resolving an id to a `LibraryHandle`, validating a
//! folder as a library, scaffolding a new one, and loading the registry from
//! `settings.json` at start-up (including the one-time migrations).
//!
//! The desktop app wraps these with a Tauri-managed `Registry` and the
//! asset-protocol scope (`src-tauri/src/infra/library.rs`); a server keeps its
//! own `Registry` and calls the same functions.

use std::path::Path;

use serde::Serialize;

use crate::config;
use crate::domain::paths;
use crate::error::{AppError, AppResult};
use crate::infra::registry::{self, LibraryEntry, LibraryHandle, LibraryKind};
use crate::infra::{fsio, settings};

/// The libraries in the order added, and the default.
#[derive(Default, Clone)]
pub struct Registry {
    pub libraries: Vec<LibraryEntry>,
    pub default: Option<String>,
}

/// What a token of a remote library may do. Ordered `read < write < admin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteRole {
    Read,
    Write,
    Admin,
}

/// How this Toybox and a library's server fit together (the comparison and its words live in
/// `toybox-remote`). `state` is `ok`, `notice` (works, versions differ), `server_too_old` or `client_too_old`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCompat {
    pub state: String,
    pub server_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_client_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_api_version: Option<u32>,
    /// A sentence for the person, when there is something to say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// What this client last learned about a remote library's server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteStatus {
    pub compat: Option<RemoteCompat>,
    pub role: RemoteRole,
    pub server_name: String,
    /// The server could not be reached the last time we tried; the cached catalog is used.
    pub offline: bool,
    pub offline_reason: Option<String>,
    /// A catalog from an earlier visit is cached, so an unreachable server still shows something.
    pub has_cache: bool,
}

/// The remote part of [`LibraryInfo`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub url: String,
    pub role: RemoteRole,
    pub server_name: String,
    pub offline: bool,
    /// Version of the server program, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    /// The oldest Toybox the server serves, when it said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_client_version: Option<String>,
    /// `ok`, `notice`, `server_too_old` or `client_too_old`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compat: Option<String>,
    /// The sentence to show about the versions, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compat_message: Option<String>,
}

impl RemoteInfo {
    pub fn new(url: String, role: RemoteRole, server_name: String, offline: bool, compat: Option<&RemoteCompat>) -> RemoteInfo {
        RemoteInfo {
            url,
            role,
            server_name,
            offline,
            server_version: compat.map(|c| c.server_version.clone()).filter(|v| !v.is_empty()),
            min_client_version: compat.and_then(|c| c.min_client_version.clone()),
            compat: compat.map(|c| c.state.clone()),
            compat_message: compat.and_then(|c| c.message.clone()),
        }
    }
}

/// What the frontend is told about a library.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInfo {
    pub id: String,
    pub name: String,
    pub kind: LibraryKind,
    /// The folder of a local library; for a remote library the local folder its
    /// downloaded files (and files being staged for an import) live in.
    pub root: String,
    /// False while the library cannot be read (an unplugged drive, a moved
    /// folder; a remote library that was never reachable). It stays registered;
    /// its cached catalog stays usable. A remote library whose server is down
    /// but which has a cached catalog is available and `offline`.
    pub available: bool,
    /// Whether Toybox may change the library's contents (import into it, delete
    /// from it, transfer into it). True for a local library whose folder accepts
    /// writes; for a remote one when its role is write or admin and it is online.
    pub can_write: bool,
    /// Whether the library's files may be edited in place (origin alignment and
    /// other tools that rewrite files). Local libraries that can be written;
    /// never a remote one, because those tools write straight into the folder.
    pub can_edit_files: bool,
    pub is_default: bool,
    /// A remote library that is shown from its cached catalog because the server
    /// cannot be reached right now.
    pub offline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offline_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteInfo>,
}

/// The capability flags of a remote library: (available, can_write, can_edit_files).
pub fn remote_capabilities(status: &RemoteStatus) -> (bool, bool, bool) {
    let available = !status.offline || status.has_cache;
    let can_write = available && !status.offline && status.role >= RemoteRole::Write;
    (available, can_write, false)
}

impl Registry {
    /// Resolve a local library id to its handle. `NoLibrary` when nothing is
    /// registered, so the frontend can route to the picker; an unknown id is a
    /// plain error. A remote library needs its connection: see [`Registry::resolve_with`].
    pub fn resolve(&self, app_data: &Path, id: &str) -> AppResult<LibraryHandle> {
        self.resolve_with(app_data, id, &|entry| Err(AppError::msg(format!("{} is a remote library and is not connected", entry.name))))
    }

    /// Like [`Registry::resolve`], with `remote` building the handle of a remote library.
    pub fn resolve_with(&self, app_data: &Path, id: &str, remote: &dyn Fn(&LibraryEntry) -> AppResult<LibraryHandle>) -> AppResult<LibraryHandle> {
        if self.libraries.is_empty() {
            return Err(AppError::NoLibrary);
        }
        let entry = self
            .libraries
            .iter()
            .find(|l| l.id == id)
            .ok_or_else(|| AppError::msg(format!("Unknown library: {id}")))?;
        match entry.kind {
            LibraryKind::Local => Ok(LibraryHandle::local(app_data, entry)),
            LibraryKind::Remote => remote(entry),
        }
    }

    pub fn list(&self) -> Vec<LibraryInfo> {
        self.list_with(&|_| None)
    }

    /// The libraries as the frontend sees them; `status` supplies what is known
    /// about each remote library (none known: not available).
    pub fn list_with(&self, status: &dyn Fn(&LibraryEntry) -> Option<RemoteStatus>) -> Vec<LibraryInfo> {
        self.libraries
            .iter()
            .map(|entry| {
                let is_default = self.default.as_deref() == Some(entry.id.as_str());
                match entry.kind {
                    LibraryKind::Local => {
                        let available = validate(Path::new(&entry.root)).is_ok();
                        let can_write = available && is_writable(Path::new(&entry.root));
                        LibraryInfo {
                            id: entry.id.clone(), name: entry.name.clone(), kind: entry.kind, root: entry.root.clone(),
                            available, can_write, can_edit_files: can_write, is_default,
                            offline: false, offline_reason: None, remote: None,
                        }
                    }
                    LibraryKind::Remote => {
                        let url = entry.url.clone().unwrap_or_default();
                        match status(entry) {
                            Some(known) => {
                                let (available, can_write, can_edit_files) = remote_capabilities(&known);
                                LibraryInfo {
                                    id: entry.id.clone(), name: entry.name.clone(), kind: entry.kind, root: entry.root.clone(),
                                    available, can_write, can_edit_files, is_default,
                                    offline: known.offline, offline_reason: known.offline_reason.clone(),
                                    remote: Some(RemoteInfo::new(url, known.role, known.server_name, known.offline, known.compat.as_ref())),
                                }
                            }
                            None => LibraryInfo {
                                id: entry.id.clone(), name: entry.name.clone(), kind: entry.kind, root: entry.root.clone(),
                                available: false, can_write: false, can_edit_files: false, is_default,
                                offline: false, offline_reason: None,
                                remote: Some(RemoteInfo::new(url, RemoteRole::Read, String::new(), false, None)),
                            },
                        }
                    }
                }
            })
            .collect()
    }
}

/// Whether the library folder accepts writes. Cheap and side-effect free: the
/// library database (or, before it is migrated, the manifest) is opened for
/// appending (nothing is written, nothing is created), which fails on a
/// read-only attribute, a read-only volume or share, and missing permission.
/// Folder-level permissions are not probed.
pub fn is_writable(root: &Path) -> bool {
    let database = paths::abs_under_root(&root.to_string_lossy(), config::LIBRARY_DB_REL);
    let file = if database.is_file() { database } else { paths::abs_under_root(&root.to_string_lossy(), config::SEED_REL) };
    std::fs::OpenOptions::new().append(true).open(file).is_ok()
}

/// Load settings from `settings_path`, migrate old single-library settings and
/// app data into the registry layout and return the registry. Never fails on an
/// unreadable settings file (it loads as empty); a migration step that fails is
/// reported on stderr and retried at the next start. The desktop app calls this
/// from `.setup()`, where an error would abort window creation.
pub fn hydrate_registry(settings_path: &Path, app_data: &Path) -> AppResult<Registry> {
    let mut stored = settings::load_from(settings_path)?;
    let migrated = registry::migrate_settings(&mut stored, registry::new_id);
    let saved = !migrated
        || settings::save_to(settings_path, &stored)
            .map_err(|error| eprintln!("toybox: could not save the library registry: {error}"))
            .is_ok();
    // Only move app data once the registry that names its owner is on disk.
    if saved {
        if let Err(error) = registry::migrate_app_data(app_data, &stored) {
            eprintln!("toybox: app-data migration will retry on the next start: {error}");
        }
        for entry in &stored.libraries {
            let _ = registry::write_sidecar(app_data, entry);
        }
    }
    Ok(Registry { libraries: stored.libraries, default: stored.default_library })
}

/// A valid library is a Toybox-curated tree: it must carry the library database
/// at `_library_config/library.db`, or the manifest `_library_config/catalog.json`
/// of a library that has not been migrated to the database yet. Anything else is
/// rejected with a message the picker shows verbatim.
pub fn validate(root: &Path) -> AppResult<()> {
    if !root.is_dir() {
        return Err(AppError::msg(format!(
            "not a folder: {}",
            root.to_string_lossy()
        )));
    }
    let database = paths::abs_under_root(&root.to_string_lossy(), config::LIBRARY_DB_REL);
    let seed = paths::abs_under_root(&root.to_string_lossy(), config::SEED_REL);
    if !fsio::exists(&database) && !fsio::exists(&seed) {
        return Err(AppError::msg(
            "not a Toybox library: missing _library_config/library.db".to_string(),
        ));
    }
    Ok(())
}

/// Establish a new, empty library in `root` unless it already is one. An
/// already-valid library is adopted as-is (idempotent); otherwise the folder
/// must be empty, so scaffolding can never clobber unrelated files. The
/// skeleton is the minimum a scan accepts: an empty seed catalog and the type
/// directories under `library/` that importers populate. Registering the
/// result is the caller's business.
pub fn ensure_library_folder(root: &Path) -> AppResult<()> {
    if validate(root).is_err() {
        require_empty_dir(root)?;
        scaffold(root)?;
    }
    Ok(())
}

/// Refuse to scaffold into a folder that already holds files. Creating into an
/// empty (or not-yet-existing) folder is fine; anything else risks surprising
/// the user, so it's an error they can act on.
fn require_empty_dir(root: &Path) -> AppResult<()> {
    if !root.exists() {
        return Ok(());
    }
    if !root.is_dir() {
        return Err(AppError::msg(format!(
            "not a folder: {}",
            root.to_string_lossy()
        )));
    }
    let mut entries = std::fs::read_dir(root)?;
    if entries.next().is_some() {
        return Err(AppError::msg(
            "folder is not empty — choose an empty folder for a new library".to_string(),
        ));
    }
    Ok(())
}

/// Lay down the minimum a library needs: a library database holding an empty
/// manifest (`_library_config/library.db`; the folder is created with it) and
/// the standard type directories. Importers stage files; the host owns storage.
fn scaffold(root: &Path) -> AppResult<()> {
    crate::infra::store::write_manifest(root, &empty_seed())?;
    for kind in ["models", "materials", "animations", "audio"] {
        std::fs::create_dir_all(root.join("library").join(kind))?;
    }
    Ok(())
}

/// The native seed source for an empty library. `schemaVersion: 1` is the
/// *source* schema the seed parser reads (distinct from the cache
/// `config::SCHEMA_VERSION`); `assets: []` is what an empty library declares.
fn empty_seed() -> serde_json::Value {
    serde_json::json!({ "schemaVersion": 1, "storageVersion": 1, "assets": [] })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::seed::parse_seed_entries;

    /// A unique temp dir for one test, removed on drop so a failed run doesn't
    /// leak. Names are derived from the test, not a clock/RNG, so they're stable.
    struct TmpDir(std::path::PathBuf);
    impl TmpDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("toybox_lib_test_{tag}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TmpDir(dir)
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn scaffold_produces_a_library_that_validates_and_scans_empty() {
        let tmp = TmpDir::new("scaffold");
        scaffold(&tmp.0).expect("scaffold");

        // The skeleton is a valid library, with the packs dir present.
        validate(&tmp.0).expect("scaffolded dir should validate");
        assert!(tmp.0.join("library").is_dir(), "library/ dir created");

        // The seed the scan reads parses as an empty asset list — no crash, no
        // phantom assets.
        let doc = crate::infra::store::read_manifest(&tmp.0).unwrap();
        assert!(parse_seed_entries(&doc).unwrap().is_empty(), "empty seed");
        assert!(!paths::abs_under_root(&tmp.0.to_string_lossy(), config::SEED_REL).exists(), "no JSON manifest is written any more");
    }

    #[test]
    fn a_remote_library_is_described_by_its_role_and_whether_the_server_answers() {
        let entry = LibraryEntry::remote("r1", "Cloud", "https://assets.example.com");
        let registry = Registry { libraries: vec![entry.clone()], default: Some("r1".into()) };
        let status = |role, offline, has_cache| RemoteStatus { compat: None, role, server_name: "My Toybox".into(), offline, offline_reason: offline.then(|| "Cannot reach the server".to_string()), has_cache };
        let list = |known: Option<RemoteStatus>| serde_json::to_value(registry.list_with(&|_| known.clone())).unwrap();

        // Online: the role decides whether it can be written; files are never edited in place.
        for (role, writable) in [(RemoteRole::Read, false), (RemoteRole::Write, true), (RemoteRole::Admin, true)] {
            let info = &list(Some(status(role, false, true)))[0];
            assert_eq!((info["available"].as_bool(), info["canWrite"].as_bool(), info["canEditFiles"].as_bool()), (Some(true), Some(writable), Some(false)), "{role:?}");
            assert_eq!(info["kind"], "remote");
            assert_eq!(info["remote"]["serverName"], "My Toybox");
            assert_eq!(info["remote"]["url"], "https://assets.example.com");
            assert_eq!(info["isDefault"], true);
        }
        // Offline with a cached catalog: still available, read-only, with a reason.
        let info = &list(Some(status(RemoteRole::Admin, true, true)))[0];
        assert_eq!((info["available"].as_bool(), info["canWrite"].as_bool(), info["offline"].as_bool()), (Some(true), Some(false), Some(true)));
        assert_eq!(info["offlineReason"], "Cannot reach the server");
        assert_eq!(info["remote"]["role"], "admin");
        // Offline and never cached: nothing to show.
        let info = &list(Some(status(RemoteRole::Write, true, false)))[0];
        assert_eq!((info["available"].as_bool(), info["canWrite"].as_bool()), (Some(false), Some(false)));
        // A remote library nobody has described yet is not available either.
        assert_eq!(list(None)[0]["available"], false);
        // A remote handle cannot be resolved without its connection.
        assert!(registry.resolve(Path::new("/app"), "r1").is_err());
    }

    #[test]
    fn require_empty_dir_accepts_empty_or_absent_and_rejects_occupied() {
        let tmp = TmpDir::new("empty");
        require_empty_dir(&tmp.0).expect("fresh empty dir is allowed");
        require_empty_dir(&tmp.0.join("does-not-exist")).expect("absent dir is allowed");

        fsio::write_text(&tmp.0.join("stray.txt"), "x").unwrap();
        assert!(require_empty_dir(&tmp.0).is_err(), "occupied dir is rejected");
    }
}
