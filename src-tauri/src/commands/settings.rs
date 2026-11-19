//! Application-settings commands the settings modal drives. The work is
//! `toybox_host::Host`; the library root has its own commands in `library`.

use tauri::AppHandle;

use crate::app_host::host;
use crate::error::AppResult;

#[tauri::command]
pub fn get_saved_exports(app: AppHandle) -> AppResult<Vec<serde_json::Value>> {
    host(&app).saved_exports()
}

#[tauri::command]
pub fn set_saved_exports(app: AppHandle, exports: Vec<serde_json::Value>) -> AppResult<()> {
    host(&app).set_saved_exports(exports)
}

/// Turn an installed plugin on or off. The plugin's files are untouched; the
/// frontend reloads its registry to apply the change. Turning a plugin on also
/// turns on what it depends on; turning one off is refused while a switched-on
/// plugin needs it. Returns the sorted ids whose switch changed.
#[tauri::command]
pub fn set_plugin_enabled(app: AppHandle, id: String, enabled: bool) -> AppResult<Vec<String>> {
    host(&app).set_plugin_enabled(&id, enabled)
}

/// The configured FBX2glTF executable path, or `null` if unset.
#[tauri::command]
pub fn get_fbx2gltf_path(app: AppHandle) -> AppResult<Option<String>> {
    host(&app).fbx2gltf_path()
}

/// Set (or, with `null`, clear) the FBX2glTF executable path. The path is
/// trusted (a local single-user tool), so this only persists, preserving other
/// settings.
#[tauri::command]
pub fn set_fbx2gltf_path(app: AppHandle, path: Option<String>) -> AppResult<()> {
    host(&app).set_fbx2gltf_path(path)
}
