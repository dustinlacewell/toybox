//! Resolution of the app-data locations where the catalog and thumbnail cache
//! live. This is the only place that asks Tauri for the app-data directory.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};

/// `%APPDATA%\com.toybox.app` (Windows) and equivalents elsewhere.
pub fn app_data_dir(app: &AppHandle) -> AppResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::msg(format!("app_data_dir: {e}")))
}

/// The catalog JSON path: `<app-data>/catalog.json`.
pub fn catalog_path(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(app_data_dir(app)?.join("catalog.json"))
}

/// The settings JSON path: `<app-data>/settings.json` (holds the library root).
pub fn settings_path(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(app_data_dir(app)?.join("settings.json"))
}

/// The thumbnail cache directory: `<app-data>/thumbs`.
pub fn thumb_dir(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(app_data_dir(app)?.join("thumbs"))
}

/// The plugins directory: `<app-data>/plugins`. Each subdirectory is one
/// installed plugin (`<id>/manifest.json` + entry module). The `plugin://`
/// protocol and the loader's source reads are jailed to this root.
pub fn plugins_dir(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(app_data_dir(app)?.join("plugins"))
}

/// The PNG path for one asset's thumbnail. The asset id contains `uid://` and
/// other unsafe characters, so it is sanitized into a flat filename.
pub fn thumb_path(app: &AppHandle, asset_id: &str) -> AppResult<PathBuf> {
    Ok(thumb_dir(app)?.join(format!("{}.png", sanitize_id(asset_id))))
}

/// Make an asset id safe as a single path segment.
pub fn sanitize_id(asset_id: &str) -> String {
    asset_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}
