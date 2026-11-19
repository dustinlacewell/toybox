//! The set of remote libraries open in this app: each opened on first use from
//! its registry entry and the token in the credential store.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use toybox_core::infra::library::{RemoteRole, RemoteStatus};
use toybox_core::infra::registry::{library_dir, new_id, LibraryEntry};
use toybox_core::infra::settings::AppSettings;

use crate::error::{RemoteError, RemoteResult};
use crate::library::{read_saved_summary, RemoteLibrary};
use crate::progress::Sink;
use crate::secrets::SecretStore;

/// An address and token that were checked against the server, ready to be saved.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub probe: crate::Probe,
    pub id: String,
    pub name: String,
}

/// Step one of adding a remote library: connect (network) and learn the server's
/// name and the role the token grants. Nothing is saved yet.
pub fn prepare(address: &str, token: &str, name: Option<&str>) -> RemoteResult<Prepared> {
    let probe = crate::probe(address, token)?;
    let name = name.map(str::trim).filter(|n| !n.is_empty()).map(String::from).unwrap_or_else(|| if probe.server_name.trim().is_empty() { "Remote library".to_string() } else { probe.server_name.clone() });
    Ok(Prepared { probe, id: new_id(), name })
}

pub struct Hub {
    store: Arc<dyn SecretStore>,
    app_data: PathBuf,
    /// `<app-cache>/libraries`: each remote library keeps its downloaded files in `<id>/files`.
    cache_root: PathBuf,
    progress: Option<Sink>,
    libraries: Mutex<HashMap<String, Arc<RemoteLibrary>>>,
}

impl Hub {
    pub fn new(store: Arc<dyn SecretStore>, app_data: PathBuf, cache_root: PathBuf, progress: Option<Sink>) -> Hub {
        Hub { store, app_data, cache_root, progress, libraries: Mutex::new(HashMap::new()) }
    }

    pub fn files_root(&self, id: &str) -> PathBuf {
        self.cache_root.join(id).join("files")
    }

    /// Where the downloaded files of every remote library sit (the asset-protocol scope).
    pub fn cache_root(&self) -> &std::path::Path {
        &self.cache_root
    }

    /// The open library for `entry`, opening it (no network) if need be.
    pub fn get(&self, entry: &LibraryEntry) -> RemoteResult<Arc<RemoteLibrary>> {
        let mut open = self.libraries.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(library) = open.get(&entry.id) {
            return Ok(library.clone());
        }
        let token = self.store.get(&entry.id)?.ok_or_else(|| {
            RemoteError::Local(format!(
                "The access token for \"{}\" is not in the system credential store (it may have been removed there). Use \"Change token…\" to enter it again.",
                entry.name
            ))
        })?;
        let files_root = self.files_root(&entry.id);
        std::fs::create_dir_all(&files_root)?;
        let library = RemoteLibrary::open(&self.app_data, &files_root, entry, &token, self.progress.clone())?;
        open.insert(entry.id.clone(), library.clone());
        Ok(library)
    }

    /// What is known about a remote library without using the network.
    pub fn status(&self, entry: &LibraryEntry) -> RemoteStatus {
        match self.get(entry) {
            Ok(library) => library.status(),
            Err(error) => {
                let dir = library_dir(&self.app_data, &entry.id);
                let (role, server_name) = read_saved_summary(&dir);
                RemoteStatus {
                    compat: None,
                    role: role.unwrap_or(RemoteRole::Read),
                    server_name,
                    offline: true,
                    offline_reason: Some(error.to_string()),
                    has_cache: toybox_core::infra::store::remote_cache_present(&dir),
                }
            }
        }
    }

    /// Keep a token for a library id. Fails with a sentence if the credential store does not work.
    pub fn store_token(&self, id: &str, token: &str) -> RemoteResult<()> {
        Ok(self.store.set(id, token)?)
    }

    /// Step two of adding a remote library: keep the token in the credential store
    /// and the address in the registry. Nothing is written to `settings` if the
    /// credential store refuses the token, and the token is taken back out if the
    /// registry refuses the entry.
    pub fn register(&self, settings: &mut AppSettings, prepared: &Prepared, token: &str) -> RemoteResult<LibraryEntry> {
        self.store.set(&prepared.id, token.trim())?;
        match settings.add_remote_library(&prepared.probe.url, &prepared.name, prepared.id.clone()) {
            Ok(entry) => Ok(entry),
            Err(error) => {
                let _ = self.store.delete(&prepared.id);
                Err(RemoteError::Local(error.to_string()))
            }
        }
    }

    /// Use a new token for an open or future library.
    pub fn replace_token(&self, id: &str, token: &str) -> RemoteResult<()> {
        self.store.set(id, token)?;
        if let Some(library) = self.libraries.lock().unwrap_or_else(|p| p.into_inner()).get(id) {
            library.set_token(token);
        }
        Ok(())
    }

    /// Forget a library that was removed: close it, delete its token from the
    /// credential store and its downloaded files. (Its catalog, favourites and
    /// pictures stay in the app data, like those of a removed local library.)
    pub fn forget(&self, id: &str) -> RemoteResult<()> {
        self.libraries.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        let result = self.store.delete(id);
        let _ = std::fs::remove_dir_all(self.cache_root.join(id));
        Ok(result?)
    }
}
