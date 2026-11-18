//! Application-settings commands the settings modal drives. Currently the
//! FBX2glTF converter path; the library root has its own commands in `library`.

use tauri::AppHandle;

use super::plugins;
use crate::error::AppResult;
use crate::infra::settings;

#[tauri::command]
pub fn get_saved_exports(app: AppHandle) -> AppResult<Vec<serde_json::Value>> {
    Ok(settings::load(&app)?.saved_exports)
}

#[tauri::command]
pub fn set_saved_exports(app: AppHandle, exports: Vec<serde_json::Value>) -> AppResult<()> {
    let mut s = settings::load(&app)?;
    s.saved_exports = exports;
    settings::save(&app, &s)
}

/// Turn an installed plugin on or off. The plugin's files are untouched; the
/// frontend reloads its registry to apply the change. Turning a plugin on also
/// turns on what it depends on; turning one off is refused while a switched-on
/// plugin needs it. Returns the sorted ids whose switch changed.
#[tauri::command]
pub fn set_plugin_enabled(app: AppHandle, id: String, enabled: bool) -> AppResult<Vec<String>> {
    let mut s = settings::load(&app)?;
    let plan = plugins::plan_plugin_enabled(&plugins::discover(&app)?, &s.disabled_plugins, &id, enabled)?;
    s.disabled_plugins = plan.disabled;
    settings::save(&app, &s)?;
    Ok(plan.changed)
}

/// The configured FBX2glTF executable path, or `null` if unset.
#[tauri::command]
pub fn get_fbx2gltf_path(app: AppHandle) -> AppResult<Option<String>> {
    Ok(settings::load(&app)?.fbx2gltf_path)
}

/// Set (or, with `null`, clear) the FBX2glTF executable path. We trust the path —
/// it's a local single-user tool — so this only persists, preserving other
/// settings.
#[tauri::command]
pub fn set_fbx2gltf_path(app: AppHandle, path: Option<String>) -> AppResult<()> {
    let mut s = settings::load(&app)?;
    s.fbx2gltf_path = path.filter(|p| !p.trim().is_empty());
    settings::save(&app, &s)
}
