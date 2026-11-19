//! The [`Host`]: what the app keeps in Tauri state, without Tauri.

use std::sync::{Arc, Mutex, MutexGuard};

use toybox_core::error::AppResult;
use toybox_core::infra::library::Registry;
use toybox_remote::progress::Sink;
use toybox_remote::secrets::{KeyringStore, SecretStore};
use toybox_remote::Hub;

use crate::fingerprint::Fingerprinter;
use crate::paths::HostPaths;
use crate::plugin_services::ToolProcesses;

pub struct Host {
    pub(crate) paths: HostPaths,
    pub(crate) registry: Mutex<Registry>,
    pub(crate) hub: Hub,
    /// Programs plugins started; stop them with [`ToolProcesses::stop_all`] on exit.
    pub tools: ToolProcesses,
    /// The background fingerprint pass.
    pub fingerprinter: Fingerprinter,
}

impl Host {
    /// Load the settings, migrate an old single-library setup into the registry
    /// layout, restore the registered libraries and create the remote hub (tokens in
    /// the operating system's credential store). `progress` receives the progress of
    /// remote-library downloads and syncs. Never fails on an unreadable
    /// `settings.json` (it loads as empty).
    pub fn open(paths: HostPaths, progress: Option<Sink>) -> AppResult<Host> {
        Self::open_with_secrets(paths, Arc::new(KeyringStore), progress)
    }

    /// Like [`Host::open`] with another place for the access tokens (tests).
    pub fn open_with_secrets(paths: HostPaths, secrets: Arc<dyn SecretStore>, progress: Option<Sink>) -> AppResult<Host> {
        let host = Self::open_lazy(paths, secrets, progress)?;
        // Remote libraries keep downloaded files in the cache; the viewer loads them from there.
        let _ = std::fs::create_dir_all(host.paths.remote_files_dir());
        Ok(host)
    }

    /// Like [`Host::open_with_secrets`] but creates no folder: running a command that only
    /// reads leaves a machine that never ran Toybox exactly as it was. Folders appear
    /// when something is first written.
    pub fn open_lazy(paths: HostPaths, secrets: Arc<dyn SecretStore>, progress: Option<Sink>) -> AppResult<Host> {
        let loaded = toybox_core::infra::library::hydrate_registry(&paths.settings_path(), &paths.app_data)?;
        let files = paths.remote_files_dir();
        let hub = Hub::new(secrets, paths.app_data.clone(), files, progress);
        Ok(Host { paths, registry: Mutex::new(loaded), hub, tools: ToolProcesses::default(), fingerprinter: Fingerprinter::default() })
    }

    pub fn paths(&self) -> &HostPaths {
        &self.paths
    }

    /// The set of open remote libraries.
    pub fn hub(&self) -> &Hub {
        &self.hub
    }

    pub(crate) fn lock_registry(&self) -> MutexGuard<'_, Registry> {
        // Original code unwrapped; a poisoned lock only means a panic elsewhere left the data intact.
        self.registry.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
