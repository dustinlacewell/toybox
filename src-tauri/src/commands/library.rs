//! Library-root commands: report the configured root and adopt a user-chosen
//! one. The frontend gates first-run on `get_library_root` (null → show the
//! folder picker) and commits a pick with `set_library_root`, which validates,
//! persists, and extends the asset-protocol scope before the catalog is scanned.

use std::path::PathBuf;

use tauri::AppHandle;

use crate::error::AppResult;
use crate::infra::library;

/// The configured library root, or `null` if none has been chosen yet.
#[tauri::command]
pub fn get_library_root(app: AppHandle) -> Option<String> {
    library::current(&app).map(|p| p.to_string_lossy().to_string())
}

/// Adopt a user-chosen folder as the library root. Errors (with a message the
/// picker shows) if the folder isn't a Toybox-style library.
#[tauri::command]
pub fn set_library_root(app: AppHandle, path: String) -> AppResult<()> {
    library::set(&app, &PathBuf::from(path))
}

/// Establish a new, empty library in the chosen folder and adopt it. The folder
/// must be empty (or already a library). Errors (with a message the picker
/// shows) otherwise.
#[tauri::command]
pub fn create_library(app: AppHandle, path: String) -> AppResult<()> {
    library::create(&app, &PathBuf::from(path))
}
