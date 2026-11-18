//! `convert_to_gltf`: the importer's FBX bridge. Shells the user-configured
//! FBX2glTF to turn one `.fbx` source into loose glTF and lands the produced
//! fileset under `library/<pack>/<category>/`. The conversion is
//! `toybox_core::ops::import_convert`; this command resolves the library, the
//! configured converter and the temp dir.

use std::path::Path;

use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::infra::{library, settings};

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
    let library_root = library::resolve(&app, &library)?.root;
    let exe = settings::load(&app)?
        .fbx2gltf_path
        .ok_or_else(|| AppError::msg("no FBX2glTF configured".to_string()))?;
    let temp = app
        .path()
        .temp_dir()
        .map_err(|e| AppError::msg(format!("temp_dir: {e}")))?;
    toybox_core::ops::import_convert::convert_to_gltf(
        &library_root,
        Path::new(&exe),
        Path::new(&src_path),
        &pack,
        &category,
        &stem,
        &temp,
    )
}
