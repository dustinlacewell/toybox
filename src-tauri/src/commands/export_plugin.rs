//! Per-asset export primitives exposed to JS plugins. These decompose what the
//! native `export_glb`/`export_copy` commands do internally into single-asset
//! callables a plugin orchestrates in a loop. The work is
//! `toybox_core::ops::export_plugin`; capability gating lives at the JS host
//! layer.

use serde_json::Value;
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::infra::library;
use toybox_core::ops::export_plugin as ops;
use toybox_core::ops::export_plugin::{PlacerAssetDto, PreparedImageDto};

/// Resolve and parse one catalog asset's glTF document.
#[tauri::command]
pub async fn read_asset_gltf(app: AppHandle, library: String, asset_id: String) -> AppResult<Value> {
    let handle = library::resolve(&app, &library)?;
    crate::commands::remote::blocking(move || ops::read_asset_gltf(&handle, &asset_id)).await
}

/// Bake one asset to a self-contained `.glb` and return its bytes.
#[tauri::command]
pub async fn assemble_glb_for_asset(app: AppHandle, library: String, asset_id: String) -> AppResult<Vec<u8>> {
    let handle = library::resolve(&app, &library)?;
    crate::commands::remote::blocking(move || ops::assemble_glb_for_asset(&handle, &asset_id)).await
}

/// Write one asset as a loose `.gltf` + `.bin` + textures under `target_dir`.
#[tauri::command]
pub async fn perform_asset_copy(
    app: AppHandle,
    library: String,
    asset_id: String,
    target_dir: String,
    stem: String,
    preserve_structure: bool,
) -> AppResult<Vec<String>> {
    let handle = library::resolve(&app, &library)?;
    crate::commands::remote::blocking(move || ops::perform_asset_copy(&handle, &asset_id, &target_dir, &stem, preserve_structure)).await
}

/// GLB-prepare arbitrary image bytes (PNG/JPEG passthrough, TGA->PNG), for a
/// JS-side exporter assembling its own container.
#[tauri::command]
pub async fn transcode_image(file_name: String, bytes: Vec<u8>) -> AppResult<PreparedImageDto> {
    ops::transcode_image(&file_name, bytes)
}

/// Read-or-empty the Godot `asset_library.json` at `library_json_path`, merge in
/// the published assets, register the recursive root folder for `sub_dir_res`,
/// and write it back.
#[tauri::command]
pub async fn placer_merge_file(
    library_json_path: String,
    sub_dir_res: String,
    assets: Vec<PlacerAssetDto>,
) -> AppResult<()> {
    ops::placer_merge_file(&library_json_path, &sub_dir_res, assets)
}

/// Plan native output using the same layout and collision rules as the writers.
#[tauri::command]
pub async fn preview_export(app: AppHandle, library: String, asset_ids: Vec<String>, format: String, preserve_structure: bool) -> AppResult<Vec<String>> {
    let handle = library::resolve(&app, &library)?;
    crate::commands::remote::blocking(move || ops::preview_export(&handle, &asset_ids, &format, preserve_structure)).await
}

/// Hash the selected file sets so saved exports can detect changes in file contents.
#[tauri::command]
pub async fn asset_fingerprints(app: AppHandle, library: String, asset_ids: Vec<String>, all_variants: bool) -> AppResult<std::collections::BTreeMap<String, String>> {
    let handle = library::resolve(&app, &library)?;
    tauri::async_runtime::spawn_blocking(move || ops::asset_fingerprints(&handle, &asset_ids, all_variants))
        .await.map_err(|e| AppError::msg(e.to_string()))?
}
