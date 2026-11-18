//! The live library registry: which libraries are registered, resolved to
//! `LibraryHandle`s that commands work through. Settings (`settings.json`) are
//! the durable copy; the managed `LibraryState` is the in-memory one, so a
//! command resolving a library never depends on re-reading a file.
//!
//! Validation, scaffolding, handle resolution and the start-up migrations are
//! in `toybox_core::infra::library`; this module adds the Tauri-managed state
//! and extends the asset-protocol scope so the webview can load files from
//! every registered root (the scope is otherwise a static allowlist that
//! wouldn't cover an arbitrary path).

use std::path::Path;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use toybox_remote::secrets::KeyringStore;
use toybox_remote::RemoteLibrary;

use crate::error::{AppError, AppResult};
use crate::infra::registry::{self, LibraryHandle};
use crate::infra::{appdata, settings};

pub use toybox_core::infra::library::{ensure_library_folder, validate, LibraryInfo, Registry};

/// Tauri-managed registry: the libraries in the order added, and the default.
#[derive(Default)]
pub struct LibraryState(pub Mutex<Registry>);

/// Tauri-managed set of open remote libraries (connections, file caches).
pub struct RemoteHub(pub toybox_remote::Hub);

/// Event name transfer progress of remote libraries is emitted under.
pub const PROGRESS_EVENT: &str = "library-progress";

/// Load settings, migrate old single-library settings and app data into the
/// registry layout, re-open the asset scope to every registered root and fill
/// the live state. Called once from `.setup()`; it never fails, because an
/// error there would abort window creation. A step that fails is retried on the
/// next start.
pub fn hydrate(app: &AppHandle) -> AppResult<()> {
    let data = appdata::app_data_dir(app)?;
    let loaded = toybox_core::infra::library::hydrate_registry(&appdata::settings_path(app)?, &data)?;
    for entry in loaded.libraries.iter().filter(|entry| entry.kind == registry::LibraryKind::Local) {
        extend_scope(app, Path::new(&entry.root));
    }
    // Remote libraries keep downloaded files in the app cache; the viewer loads them from there.
    let cache = appdata::remote_files_dir(app)?;
    let _ = std::fs::create_dir_all(&cache);
    extend_scope(app, &cache);
    let events = app.clone();
    let sink: toybox_remote::progress::Sink = Arc::new(move |progress| {
        let _ = events.emit(PROGRESS_EVENT, progress);
    });
    app.manage(RemoteHub(toybox_remote::Hub::new(Arc::new(KeyringStore), data, cache, Some(sink))));
    *app.state::<LibraryState>().0.lock().unwrap() = loaded;
    Ok(())
}

pub fn snapshot(app: &AppHandle) -> Registry {
    app.state::<LibraryState>().0.lock().unwrap().clone()
}

/// Resolve a library id to its handle. `NoLibrary` when nothing is registered,
/// so the frontend can route to the picker; an unknown id is a plain error.
pub fn resolve(app: &AppHandle, id: &str) -> AppResult<LibraryHandle> {
    let registry = snapshot(app);
    if registry.libraries.is_empty() {
        return Err(AppError::NoLibrary);
    }
    let hub = app.state::<RemoteHub>();
    registry.resolve_with(&appdata::app_data_dir(app)?, id, &|entry| Ok(hub.0.get(entry).map_err(AppError::from)?.handle()))
}

/// The open remote library `id`, or an error when it is a local one.
pub fn remote(app: &AppHandle, id: &str) -> AppResult<Arc<RemoteLibrary>> {
    let registry = snapshot(app);
    let entry = registry.libraries.iter().find(|l| l.id == id).ok_or_else(|| AppError::msg(format!("Unknown library: {id}")))?;
    if entry.kind != registry::LibraryKind::Remote {
        return Err(AppError::msg(format!("{} is not a remote library", entry.name)));
    }
    Ok(app.state::<RemoteHub>().0.get(entry).map_err(AppError::from)?)
}

/// The open remote library `id`, or `None` for a local library.
pub fn remote_if(app: &AppHandle, id: &str) -> AppResult<Option<Arc<RemoteLibrary>>> {
    let is_remote = snapshot(app).libraries.iter().any(|l| l.id == id && l.kind == registry::LibraryKind::Remote);
    if is_remote { remote(app, id).map(Some) } else { Ok(None) }
}

/// The libraries as the frontend sees them. A remote library is described by what
/// is known without using the network (role and offline state from the last visit).
pub fn list(app: &AppHandle) -> Vec<LibraryInfo> {
    let hub = app.state::<RemoteHub>();
    let mut infos = snapshot(app).list_with(&|entry| Some(hub.0.status(entry)));
    for info in &mut infos {
        if info.kind == registry::LibraryKind::Remote {
            info.root = hub.0.files_root(&info.id).to_string_lossy().to_string();
        }
    }
    infos
}

fn info(app: &AppHandle, id: &str) -> AppResult<LibraryInfo> {
    list(app).into_iter().find(|l| l.id == id).ok_or_else(|| AppError::msg("Unknown library"))
}

/// Apply one registry change: start from the live registry (so an unreadable
/// `settings.json` can never empty it), persist, then publish to the live state.
/// The state lock is held throughout, serialising registry changes.
fn mutate<T>(app: &AppHandle, change: impl FnOnce(&mut settings::AppSettings) -> AppResult<T>) -> AppResult<T> {
    let state = app.state::<LibraryState>();
    let mut live = state.0.lock().unwrap();
    let mut stored = settings::load(app)?;
    stored.libraries = live.libraries.clone();
    stored.default_library = live.default.clone();
    let result = change(&mut stored)?;
    settings::save(app, &stored)?;
    *live = Registry { libraries: stored.libraries, default: stored.default_library };
    Ok(result)
}

/// Register an existing library folder. A folder that was registered before
/// and removed gets its previous id (and so its kept favourites, tags and
/// thumbnails) back.
pub fn add(app: &AppHandle, root: &Path, name: Option<&str>) -> AppResult<LibraryInfo> {
    validate(root)?;
    let root_text = root.to_string_lossy().to_string();
    let data = appdata::app_data_dir(app)?;
    let entry = mutate(app, |stored| {
        let id = registry::retained_id_for(&data, stored, &root_text).unwrap_or_else(registry::new_id);
        stored.add_library(&root_text, name, id)
    })?;
    let _ = registry::write_sidecar(&data, &entry);
    extend_scope(app, root);
    info(app, &entry.id)
}

/// Establish a new, empty library in `root`, then register it. An already-valid
/// library is adopted as-is (idempotent); otherwise the folder must be empty, so
/// scaffolding can never clobber unrelated files.
pub fn create(app: &AppHandle, root: &Path, name: Option<&str>) -> AppResult<LibraryInfo> {
    ensure_library_folder(root)?;
    add(app, root, name)
}

pub fn rename(app: &AppHandle, id: &str, name: &str) -> AppResult<LibraryInfo> {
    let entry = mutate(app, |stored| stored.rename_library(id, name))?;
    if let Ok(data) = appdata::app_data_dir(app) {
        let _ = registry::write_sidecar(&data, &entry);
    }
    info(app, id)
}

/// Unregister a library. Its folder is not touched and its app data (cache with
/// favourites and tags, thumbnails) is kept under `<app-data>/libraries/<id>`,
/// so adding the same folder again restores them.
pub fn remove(app: &AppHandle, id: &str) -> AppResult<()> {
    let removed = mutate(app, |stored| stored.remove_library(id))?;
    if removed.kind == registry::LibraryKind::Remote {
        // The token leaves the credential store and the downloaded files go; the
        // catalog, favourites and pictures stay in the app data like for any library.
        app.state::<RemoteHub>().0.forget(id).map_err(AppError::from)?;
    }
    Ok(())
}

/// Add a remote library: the address and token were checked against the server
/// (`prepared`), the token goes to the credential store, the address to the registry.
pub fn add_remote(app: &AppHandle, prepared: &toybox_remote::hub::Prepared, token: &str) -> AppResult<LibraryInfo> {
    let hub = app.state::<RemoteHub>();
    let data = appdata::app_data_dir(app)?;
    let entry = match mutate(app, |stored| hub.0.register(stored, prepared, token).map_err(AppError::from)) {
        Ok(entry) => entry,
        Err(error) => {
            // The registry could not be saved: do not leave the token behind.
            let _ = hub.0.forget(&prepared.id);
            return Err(error);
        }
    };
    let _ = registry::write_sidecar(&data, &entry);
    hub.0.get(&entry).map_err(AppError::from)?.remember(&prepared.probe.server_name, prepared.probe.role);
    info(app, &entry.id)
}

/// Use another access token for a remote library, after checking it with the server.
pub fn change_token(app: &AppHandle, id: &str, token: &str) -> AppResult<LibraryInfo> {
    let library = remote(app, id)?;
    let probe = toybox_remote::probe(library.url(), token).map_err(AppError::from)?;
    app.state::<RemoteHub>().0.replace_token(id, token.trim()).map_err(AppError::from)?;
    library.remember(&probe.server_name, probe.role);
    info(app, id)
}

pub fn info_of(app: &AppHandle, id: &str) -> AppResult<LibraryInfo> {
    info(app, id)
}

pub fn set_default(app: &AppHandle, id: &str) -> AppResult<()> {
    mutate(app, |stored| stored.set_default_library(id))
}

/// Allow the webview's asset protocol to serve files under `root`. Without this
/// the catalog scans fine but every gltf/texture/thumbnail 404s in the viewer.
fn extend_scope(app: &AppHandle, root: &Path) {
    // Best-effort: a scope error here shouldn't abort adopting the library; the
    // viewer would degrade, but scanning/exports still work.
    let _ = app.asset_protocol_scope().allow_directory(root, true);
}
