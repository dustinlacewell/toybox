//! The runtime library root: the single live source of truth for "where the
//! asset library lives." It replaces the former compile-time `config::LIBRARY_ROOT`
//! const so the location is the user's choice, persisted in `settings.json` and
//! held in Tauri-managed state.
//!
//! Three responsibilities live here because they are inseparable: validating that
//! a folder is a usable library, persisting the choice, and extending the
//! asset-protocol scope so the webview can actually load files from it (the
//! scope is otherwise a static allowlist that wouldn't cover an arbitrary path).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::config;
use crate::domain::paths;
use crate::error::{AppError, AppResult};
use crate::infra::{fsio, settings};

/// Tauri-managed live root. `None` until the user has chosen a library.
#[derive(Default)]
pub struct LibraryState(pub Mutex<Option<PathBuf>>);

/// Hydrate the live root from persisted settings and, if one is present and
/// valid, extend the asset-protocol scope to it. Called once from `.setup()`.
/// A persisted-but-now-missing root is dropped (left unset) so the app falls
/// back to the picker rather than erroring on a stale path.
pub fn hydrate(app: &AppHandle) -> AppResult<()> {
    let Some(root) = settings::load(app)?.library_root.map(PathBuf::from) else {
        return Ok(());
    };
    if validate(&root).is_err() {
        return Ok(());
    }
    extend_scope(app, &root);
    store(app, Some(root));
    Ok(())
}

/// The current root, or `AppError::NoLibrary` if none is configured. Every file
/// read/write keyed to the library funnels through this, so an unconfigured app
/// fails with a distinguishable sentinel rather than a raw IO error.
pub fn resolve(app: &AppHandle) -> AppResult<PathBuf> {
    current(app).ok_or(AppError::NoLibrary)
}

/// The current root without erroring — for the picker gate (`get_library_root`).
pub fn current(app: &AppHandle) -> Option<PathBuf> {
    app.state::<LibraryState>().0.lock().unwrap().clone()
}

/// Adopt a user-chosen folder: validate it is a Toybox-style library, persist
/// the choice, extend the asset-protocol scope, and update the live root.
pub fn set(app: &AppHandle, root: &Path) -> AppResult<()> {
    validate(root)?;
    let root = root.to_path_buf();
    settings::save(
        app,
        &settings::AppSettings { library_root: Some(root.to_string_lossy().to_string()) },
    )?;
    extend_scope(app, &root);
    store(app, Some(root));
    Ok(())
}

/// A valid library is a Toybox-curated tree: it must carry the native catalog
/// seed at `_library_config/catalog.json`. Anything else is rejected with a
/// message the picker shows verbatim.
fn validate(root: &Path) -> AppResult<()> {
    if !root.is_dir() {
        return Err(AppError::msg(format!(
            "not a folder: {}",
            root.to_string_lossy()
        )));
    }
    let seed = paths::abs_under_root(&root.to_string_lossy(), config::SEED_REL);
    if !fsio::exists(&seed) {
        return Err(AppError::msg(
            "not a Toybox library: missing _library_config/catalog.json".to_string(),
        ));
    }
    Ok(())
}

/// Allow the webview's asset protocol to serve files under `root`. Without this
/// the catalog scans fine but every gltf/texture/thumbnail 404s in the viewer.
fn extend_scope(app: &AppHandle, root: &Path) {
    // Best-effort: a scope error here shouldn't abort adopting the library; the
    // viewer would degrade, but scanning/exports still work.
    let _ = app.asset_protocol_scope().allow_directory(root, true);
}

fn store(app: &AppHandle, root: Option<PathBuf>) {
    *app.state::<LibraryState>().0.lock().unwrap() = root;
}
