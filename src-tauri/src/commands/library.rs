//! Library-registry commands: list the registered libraries and add, create,
//! rename, remove or default one. Every other command takes the `library` id
//! these hand out. The frontend shows the first-run picker when the list is
//! empty.

use std::path::PathBuf;

use tauri::AppHandle;

use crate::error::AppResult;
use crate::infra::library::{self, LibraryInfo};

#[tauri::command]
pub fn list_libraries(app: AppHandle) -> Vec<LibraryInfo> {
    library::list(&app)
}

/// Register an existing folder as a library. Errors (with a message the picker
/// shows) if the folder isn't a Toybox-style library or is already registered.
/// `name` defaults to the folder name.
#[tauri::command]
pub fn add_library(app: AppHandle, path: String, name: Option<String>) -> AppResult<LibraryInfo> {
    library::add(&app, &PathBuf::from(path), name.as_deref())
}

/// Establish a new, empty library in the chosen folder and register it. The
/// folder must be empty (or already a library). Errors otherwise.
#[tauri::command]
pub fn create_library(app: AppHandle, path: String, name: Option<String>) -> AppResult<LibraryInfo> {
    library::create(&app, &PathBuf::from(path), name.as_deref())
}

#[tauri::command]
pub fn rename_library(app: AppHandle, library: String, name: String) -> AppResult<LibraryInfo> {
    library::rename(&app, &library, &name)
}

/// Unregister a library. Its folder and files are left alone.
#[tauri::command]
pub fn remove_library(app: AppHandle, library: String) -> AppResult<()> {
    library::remove(&app, &library)
}

#[tauri::command]
pub fn set_default_library(app: AppHandle, library: String) -> AppResult<()> {
    library::set_default(&app, &library)
}
