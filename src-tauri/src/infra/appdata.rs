//! Resolution of the app-data locations. This is the only place that asks Tauri
//! for the app-data directory. Per-library data (catalog cache, thumbnails) lives
//! under `<app-data>/libraries/<id>/`; see `infra::registry`.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};

/// `%APPDATA%\com.toybox.app` (Windows) and equivalents elsewhere.
pub fn app_data_dir(app: &AppHandle) -> AppResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::msg(format!("app_data_dir: {e}")))
}

/// Where downloaded files of remote libraries live: `<app-cache>/libraries`. A cache
/// the OS may clear and that can always be fetched again, unlike the app data.
pub fn remote_files_dir(app: &AppHandle) -> AppResult<PathBuf> {
    app.path()
        .app_cache_dir()
        .map(|dir| dir.join("libraries"))
        .map_err(|e| AppError::msg(format!("app_cache_dir: {e}")))
}

/// The settings JSON path: `<app-data>/settings.json` (holds the library registry).
pub fn settings_path(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(toybox_core::infra::appdata::settings_path(&app_data_dir(app)?))
}

/// The plugins directory: `<app-data>/plugins`. Each subdirectory is one
/// installed plugin (`<id>/manifest.json` + entry module). The `plugin://`
/// protocol and the loader's source reads are jailed to this root.
pub fn plugins_dir(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(app_data_dir(app)?.join("plugins"))
}
