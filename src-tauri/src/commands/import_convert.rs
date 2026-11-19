//! `convert_to_gltf`: the importer's FBX bridge. Shells the user-configured
//! FBX2glTF to turn one `.fbx` source into loose glTF and lands the produced
//! fileset under `library/<pack>/<category>/`. The work is
//! `toybox_host::Host::convert_to_gltf`.

use tauri::AppHandle;

use crate::app_host::host;
use crate::error::AppResult;

/// Errors with a clear message if no converter is configured.
#[tauri::command]
pub async fn convert_to_gltf(
    app: AppHandle,
    library: String,
    src_path: String,
    pack: String,
    category: String,
    stem: String,
) -> AppResult<Vec<String>> {
    host(&app).convert_to_gltf(&library, &src_path, &pack, &category, &stem)
}
