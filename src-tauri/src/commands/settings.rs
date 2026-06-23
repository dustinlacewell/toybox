//! Application-settings commands the settings modal drives. Currently the
//! FBX2glTF converter path; the library root has its own commands in `library`.

use tauri::AppHandle;

use crate::error::AppResult;
use crate::infra::settings;

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
