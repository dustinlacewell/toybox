//! The data directory: everything the server keeps, in one folder you can back
//! up by copying.
//!
//! ```text
//! <data>/
//!   server.json   name and tokens (hashes only)
//!   library/      the Toybox library, in the ordinary on-disk format
//!   cache/        catalog cache and shared thumbnails (rebuildable)
//!   uploads/      staging area for imports in progress (temporary)
//! ```

use std::path::{Path, PathBuf};

use toybox_core::infra::registry::{LibraryHandle, LibraryKind};

use crate::config::{ConfigError, Role, ServerConfig, TokenRecord};

#[derive(Debug, Clone)]
pub struct DataDir {
    pub root: PathBuf,
}

impl DataDir {
    pub fn new(root: impl Into<PathBuf>) -> DataDir {
        DataDir { root: root.into() }
    }
    pub fn server_json(&self) -> PathBuf {
        self.root.join("server.json")
    }
    pub fn library(&self) -> PathBuf {
        self.root.join("library")
    }
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn uploads(&self) -> PathBuf {
        self.root.join("uploads")
    }
    pub fn is_initialised(&self) -> bool {
        self.server_json().is_file()
    }

    /// The core handle for the served library: the library folder, a catalog
    /// cache and a thumbnail folder under `cache/`.
    pub fn handle(&self) -> LibraryHandle {
        let cache = self.cache();
        LibraryHandle {
            id: "server".to_string(),
            kind: LibraryKind::Local,
            root: self.library(),
            cache_path: cache.join("catalog.json"),
            thumbs_dir: cache.join("thumbs"),
            remote: None,
        }
    }
}

pub struct InitOutcome {
    pub record: TokenRecord,
    pub secret: String,
    pub library_was_empty: bool,
}

/// Set up a data directory: scaffold (or adopt) the library, create the
/// folders, write `server.json` with one admin token. Refuses to run twice.
pub fn init(data: &DataDir, name: &str) -> Result<InitOutcome, String> {
    if data.is_initialised() {
        return Err(format!(
            "{} is already set up. Use 'toybox-server token add' to create more tokens.",
            data.root.display()
        ));
    }
    std::fs::create_dir_all(&data.root).map_err(|e| format!("Could not create {}: {e}", data.root.display()))?;
    for dir in [data.cache(), data.uploads()] {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    }
    let library = data.library();
    let had_library = toybox_core::infra::library::validate(&library).is_ok();
    toybox_core::infra::library::ensure_library_folder(&library).map_err(|e| e.to_string())?;
    let mut config = ServerConfig::new(name);
    let (record, secret) = config.add_token("admin", Role::Admin).map_err(|e: ConfigError| e.to_string())?;
    // Index the library now so the first start is quick and a broken library is reported here.
    let handle = data.handle();
    let catalog = toybox_core::ops::scan::scan_library_catalog(&handle).map_err(|e| e.to_string())?;
    config.save(&data.server_json()).map_err(|e| e.to_string())?;
    Ok(InitOutcome { record, secret, library_was_empty: !had_library || catalog.assets.is_empty() })
}

pub fn display_path(path: &Path) -> String {
    path.display().to_string()
}
