//! Library-registry commands: list the registered libraries and add, create,
//! rename, remove or default one. Every other command takes the `library` id
//! these hand out. The frontend shows the first-run picker when the list is
//! empty. The work is `toybox_host::Host`; after adding a library the window
//! opens its asset-protocol scope to the folder.

use std::path::PathBuf;

use tauri::AppHandle;
use toybox_host::libraries::LibraryInfo;

use crate::app_host::{extend_scope, host};
use crate::error::AppResult;

#[tauri::command]
pub fn list_libraries(app: AppHandle) -> Vec<LibraryInfo> {
    host(&app).list_libraries()
}

/// Register an existing folder as a library. Errors (with a message the picker
/// shows) if the folder isn't a Toybox-style library or is already registered.
/// `name` defaults to the folder name.
#[tauri::command]
pub fn add_library(app: AppHandle, path: String, name: Option<String>) -> AppResult<LibraryInfo> {
    let root = PathBuf::from(path);
    let info = host(&app).add_library(&root, name.as_deref())?;
    extend_scope(&app, &root);
    Ok(info)
}

/// Establish a new, empty library in the chosen folder and register it. The
/// folder must be empty (or already a library). Errors otherwise.
#[tauri::command]
pub fn create_library(app: AppHandle, path: String, name: Option<String>) -> AppResult<LibraryInfo> {
    let root = PathBuf::from(path);
    let info = host(&app).create_library(&root, name.as_deref())?;
    extend_scope(&app, &root);
    Ok(info)
}

#[tauri::command]
pub fn rename_library(app: AppHandle, library: String, name: String) -> AppResult<LibraryInfo> {
    host(&app).rename_library(&library, &name)
}

/// Unregister a library. Its folder and files are left alone.
#[tauri::command]
pub fn remove_library(app: AppHandle, library: String) -> AppResult<()> {
    host(&app).remove_library(&library)
}

#[tauri::command]
pub fn set_default_library(app: AppHandle, library: String) -> AppResult<()> {
    host(&app).set_default_library(&library)
}
