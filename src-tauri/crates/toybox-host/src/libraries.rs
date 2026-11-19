//! The live library registry: which libraries are registered, resolved to
//! `LibraryHandle`s that operations work through. `settings.json` is the durable
//! copy; the registry inside the [`Host`] is the in-memory one, so resolving a
//! library never depends on re-reading a file.
//!
//! Validation, scaffolding, handle resolution and the start-up migrations are in
//! `toybox_core::infra::library`; this adds the live state and the hub of remote
//! libraries. Extending the webview's asset-protocol scope after a library is
//! added is the window's step, done by the app.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use toybox_core::error::{AppError, AppResult};
use toybox_core::infra::library::{ensure_library_folder, validate};
pub use toybox_core::infra::library::{LibraryInfo, Registry};
use toybox_core::infra::registry::{self, LibraryHandle};
use toybox_core::infra::settings::AppSettings;
use toybox_remote::RemoteLibrary;

use crate::host::Host;

impl Host {
    /// A copy of the registry: the libraries in the order added, and the default.
    pub fn registry(&self) -> Registry {
        self.lock_registry().clone()
    }

    /// Roots of the registered local libraries (the window opens its asset scope to them).
    pub fn local_library_roots(&self) -> Vec<PathBuf> {
        self.registry().libraries.iter().filter(|entry| entry.kind == registry::LibraryKind::Local).map(|entry| PathBuf::from(&entry.root)).collect()
    }

    /// Resolve a library id to its handle. `NoLibrary` when nothing is registered,
    /// so the frontend can route to the picker; an unknown id is a plain error.
    pub fn resolve(&self, id: &str) -> AppResult<LibraryHandle> {
        let registry = self.registry();
        if registry.libraries.is_empty() {
            return Err(AppError::NoLibrary);
        }
        registry.resolve_with(&self.paths.app_data, id, &|entry| Ok(self.hub.get(entry).map_err(AppError::from)?.handle()))
    }

    /// The open remote library `id`, or an error when it is a local one.
    pub fn remote(&self, id: &str) -> AppResult<Arc<RemoteLibrary>> {
        let registry = self.registry();
        let entry = registry.libraries.iter().find(|l| l.id == id).ok_or_else(|| AppError::msg(format!("Unknown library: {id}")))?;
        if entry.kind != registry::LibraryKind::Remote {
            return Err(AppError::msg(format!("{} is not a remote library", entry.name)));
        }
        Ok(self.hub.get(entry).map_err(AppError::from)?)
    }

    /// The open remote library `id`, or `None` for a local library.
    pub fn remote_if(&self, id: &str) -> AppResult<Option<Arc<RemoteLibrary>>> {
        let is_remote = self.registry().libraries.iter().any(|l| l.id == id && l.kind == registry::LibraryKind::Remote);
        if is_remote { self.remote(id).map(Some) } else { Ok(None) }
    }

    /// The libraries as the frontend sees them. A remote library is described by what
    /// is known without using the network (role and offline state from the last visit).
    pub fn list_libraries(&self) -> Vec<LibraryInfo> {
        let mut infos = self.registry().list_with(&|entry| Some(self.hub.status(entry)));
        for info in &mut infos {
            if info.kind == registry::LibraryKind::Remote {
                info.root = self.hub.files_root(&info.id).to_string_lossy().to_string();
            }
        }
        infos
    }

    /// One registered library as listed; an error for an unknown id.
    pub fn library_info(&self, id: &str) -> AppResult<LibraryInfo> {
        self.list_libraries().into_iter().find(|l| l.id == id).ok_or_else(|| AppError::msg("Unknown library"))
    }

    /// Apply one registry change: start from the live registry (so an unreadable
    /// `settings.json` can never empty it), persist, then publish to the live state.
    /// The registry lock is held throughout, serialising registry changes.
    fn mutate<T>(&self, change: impl FnOnce(&mut AppSettings) -> AppResult<T>) -> AppResult<T> {
        let mut live = self.lock_registry();
        let mut stored = self.load_settings()?;
        stored.libraries = live.libraries.clone();
        stored.default_library = live.default.clone();
        let result = change(&mut stored)?;
        self.save_settings(&stored)?;
        *live = Registry { libraries: stored.libraries, default: stored.default_library };
        Ok(result)
    }

    /// Register an existing library folder. A folder that was registered before
    /// and removed gets its previous id (and so its kept favourites, tags and
    /// thumbnails) back.
    pub fn add_library(&self, root: &Path, name: Option<&str>) -> AppResult<LibraryInfo> {
        validate(root)?;
        let root_text = root.to_string_lossy().to_string();
        let data = &self.paths.app_data;
        let entry = self.mutate(|stored| {
            let id = registry::retained_id_for(data, stored, &root_text).unwrap_or_else(registry::new_id);
            stored.add_library(&root_text, name, id)
        })?;
        let _ = registry::write_sidecar(data, &entry);
        self.library_info(&entry.id)
    }

    /// Establish a new, empty library in `root`, then register it. An already-valid
    /// library is adopted as-is (idempotent); otherwise the folder must be empty, so
    /// scaffolding can never clobber unrelated files.
    pub fn create_library(&self, root: &Path, name: Option<&str>) -> AppResult<LibraryInfo> {
        ensure_library_folder(root)?;
        self.add_library(root, name)
    }

    pub fn rename_library(&self, id: &str, name: &str) -> AppResult<LibraryInfo> {
        let entry = self.mutate(|stored| stored.rename_library(id, name))?;
        let _ = registry::write_sidecar(&self.paths.app_data, &entry);
        self.library_info(id)
    }

    /// Unregister a library. Its folder is not touched and its app data (cache with
    /// favourites and tags, thumbnails) is kept under `<app-data>/libraries/<id>`,
    /// so adding the same folder again restores them. For a remote library the token
    /// leaves the credential store and the downloaded files go.
    pub fn remove_library(&self, id: &str) -> AppResult<()> {
        let removed = self.mutate(|stored| stored.remove_library(id))?;
        if removed.kind == registry::LibraryKind::Remote {
            self.hub.forget(id).map_err(AppError::from)?;
        }
        Ok(())
    }

    pub fn set_default_library(&self, id: &str) -> AppResult<()> {
        self.mutate(|stored| stored.set_default_library(id))
    }

    /// Add a remote library: the address and token were checked against the server
    /// (`prepared`), the token goes to the credential store, the address to the registry.
    pub fn add_remote_prepared(&self, prepared: &toybox_remote::hub::Prepared, token: &str) -> AppResult<LibraryInfo> {
        let entry = match self.mutate(|stored| self.hub.register(stored, prepared, token).map_err(AppError::from)) {
            Ok(entry) => entry,
            Err(error) => {
                // The registry could not be saved: do not leave the token behind.
                let _ = self.hub.forget(&prepared.id);
                return Err(error);
            }
        };
        let _ = registry::write_sidecar(&self.paths.app_data, &entry);
        self.hub.get(&entry).map_err(AppError::from)?.remember(&prepared.probe.server_name, prepared.probe.role);
        self.library_info(&entry.id)
    }

    /// Check `address` and `token` with the server (network), then register the
    /// library. The token goes to the credential store, never to settings.
    pub fn add_remote_library(&self, address: &str, token: &str, name: Option<&str>) -> AppResult<LibraryInfo> {
        let prepared = toybox_remote::hub::prepare(address, token, name).map_err(AppError::from)?;
        self.add_remote_prepared(&prepared, token.trim())
    }

    /// Use another access token for a remote library, after checking it with the server.
    pub fn change_remote_token(&self, id: &str, token: &str) -> AppResult<LibraryInfo> {
        let library = self.remote(id)?;
        let probe = toybox_remote::probe(library.url(), token).map_err(AppError::from)?;
        self.hub.replace_token(id, token.trim()).map_err(AppError::from)?;
        library.remember(&probe.server_name, probe.role);
        self.library_info(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::HostPaths;
    use std::sync::atomic::{AtomicU32, Ordering};
    use toybox_remote::secrets::MemoryStore;

    fn temp(name: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("toybox-host-{name}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open(home: &Path) -> Host {
        Host::open_with_secrets(HostPaths::for_home(home), Arc::new(MemoryStore::new()), None).unwrap()
    }

    #[test]
    fn libraries_are_created_listed_renamed_defaulted_and_forgotten_across_restarts() {
        let home = temp("libs");
        let host = open(&home);
        assert!(host.list_libraries().is_empty());
        assert!(matches!(host.resolve("x"), Err(AppError::NoLibrary)));

        let root = home.join("lib-one");
        let info = host.create_library(&root, Some("One")).unwrap();
        assert_eq!((info.name.as_str(), info.available), ("One", true));
        assert_eq!(host.local_library_roots(), vec![root.clone()]);
        assert!(host.resolve(&info.id).is_ok());
        assert!(host.remote_if(&info.id).unwrap().is_none());
        assert!(host.remote(&info.id).is_err());
        assert!(host.resolve("nope").is_err());

        host.rename_library(&info.id, "Uno").unwrap();
        host.set_default_library(&info.id).unwrap();

        // A second host over the same folders sees the same registry.
        let again = open(&home);
        let listed = again.list_libraries();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].name.as_str(), listed[0].is_default), ("Uno", true));

        again.remove_library(&info.id).unwrap();
        assert!(again.list_libraries().is_empty());
        // Adding the same folder back restores its id.
        let back = again.add_library(&root, None).unwrap();
        assert_eq!(back.id, info.id);
        let _ = std::fs::remove_dir_all(&home);
    }
}
