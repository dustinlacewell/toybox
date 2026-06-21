//! Per-asset export primitives exposed to JS plugins. These decompose what the
//! native `export_glb`/`export_copy`/`export_placer` commands do internally into
//! single-asset callables a plugin orchestrates in a loop:
//!
//! - `read_asset_gltf`        — the parsed glTF JSON for one asset.
//! - `assemble_glb_for_asset` — bake one asset to self-contained GLB bytes.
//! - `perform_asset_copy`     — write one asset's loose `.gltf`+`.bin`+textures.
//! - `transcode_image`        — GLB-prepare arbitrary image bytes (PNG passthrough,
//!                              TGA->PNG).
//! - `placer_merge_file`      — create/merge a Godot `asset_library.json`. The
//!                              banding/idempotent-merge logic stays in
//!                              `domain::placer_library` (its unit tests still cover it).
//!
//! Each reuses the same domain functions the native exporters use, so a plugin
//! that leans on these produces byte-identical output. Capability gating lives at
//! the JS host layer; the real filesystem boundary is the target dir the user
//! picked, which the plugin passes straight through.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::domain::export_copy_plan::plan_copy;
use crate::domain::export_model::ExportReport;
use crate::domain::glb_assemble::assemble;
use crate::domain::image_embed::prepare_for_glb;
use crate::domain::placer_library::{self, PlacerAsset};
use crate::error::{AppError, AppResult};
use crate::infra::fsio;

use super::export_glb::gather_images;
use super::export_util::{
    collect_assets, execute_copy_plan, library_root, read_bin, read_gltf,
};

/// Resolve and parse one catalog asset's glTF document.
#[tauri::command]
pub async fn read_asset_gltf(app: AppHandle, asset_id: String) -> AppResult<Value> {
    let asset = one_asset(&app, &asset_id)?;
    read_gltf(library_root(), &asset)
}

/// Bake one asset to a self-contained `.glb` and return its bytes. Reuses the
/// same gltf/bin/image gather + `assemble` path as `export_glb`.
#[tauri::command]
pub async fn assemble_glb_for_asset(app: AppHandle, asset_id: String) -> AppResult<Vec<u8>> {
    let asset = one_asset(&app, &asset_id)?;
    let root = library_root();
    let doc = read_gltf(root, &asset)?;
    let bin = read_bin(root, &asset)?;
    let mut report = ExportReport::default(); // image warnings are returned separately when needed
    let images = gather_images(root, &asset, &doc, &mut report)?;
    assemble(&doc, &bin, images)
}

/// Write one asset as a loose `.gltf` + `.bin` + textures under `target_dir`,
/// using the chosen `stem` and layout. Returns the target-relative paths
/// written. Reuses `plan_copy` + the shared `execute_copy_plan` writer.
#[tauri::command]
pub async fn perform_asset_copy(
    app: AppHandle,
    asset_id: String,
    target_dir: String,
    stem: String,
    preserve_structure: bool,
) -> AppResult<Vec<String>> {
    let asset = one_asset(&app, &asset_id)?;
    let root = library_root();
    let doc = read_gltf(root, &asset)?;
    let plan = plan_copy(
        &doc,
        &asset.fileset.gltf,
        &asset.fileset.bin,
        &asset.pack,
        &asset.category,
        &stem,
        preserve_structure,
    )?;
    let mut report = ExportReport::default();
    let mut copied = HashSet::new();
    execute_copy_plan(root, Path::new(&target_dir), &plan, &mut copied, &mut report)?;
    Ok(report.written)
}

/// GLB-prepare arbitrary image bytes (PNG/JPEG passthrough, TGA->PNG), for a
/// JS-side exporter assembling its own container.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedImageDto {
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[tauri::command]
pub async fn transcode_image(file_name: String, bytes: Vec<u8>) -> AppResult<PreparedImageDto> {
    let prepared = prepare_for_glb(&file_name, bytes)?;
    Ok(PreparedImageDto { mime: prepared.mime, bytes: prepared.bytes })
}

/// One asset to publish into a Godot `asset_library.json`. Mirrors
/// `placer_library::PlacerAsset` over the command boundary.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacerAssetDto {
    pub pack: String,
    pub category: String,
    pub favorite: bool,
    pub tags: Vec<String>,
    pub res_path: String,
    pub name: String,
}

impl From<PlacerAssetDto> for PlacerAsset {
    fn from(d: PlacerAssetDto) -> Self {
        PlacerAsset {
            pack: d.pack,
            category: d.category,
            favorite: d.favorite,
            tags: d.tags,
            res_path: d.res_path,
            name: d.name,
        }
    }
}

/// Read-or-empty the Godot `asset_library.json` at `library_json_path`, merge in
/// the published assets, register the recursive root folder for `sub_dir_res`,
/// and write it back. The banding/idempotency logic is `placer_library`'s — this
/// is the same orchestration the old `export_placer::merge_library` performed.
#[tauri::command]
pub async fn placer_merge_file(
    library_json_path: String,
    sub_dir_res: String,
    assets: Vec<PlacerAssetDto>,
) -> AppResult<()> {
    let placer_assets: Vec<PlacerAsset> = assets.into_iter().map(Into::into).collect();
    let lib_path = Path::new(&library_json_path);
    let existing = if fsio::exists(lib_path) {
        let text = fsio::read_text(lib_path)?;
        serde_json::from_str(&text).unwrap_or_else(|_| placer_library::empty_library())
    } else {
        placer_library::empty_library()
    };
    let mut merged = placer_library::merge(&existing, &placer_assets);
    placer_library::ensure_folder(&mut merged, &sub_dir_res);
    fsio::write_text(lib_path, &serde_json::to_string_pretty(&merged)?)
}

/// Resolve a single asset by id (a one-element `collect_assets`), erroring if
/// the id is unknown rather than silently skipping it.
fn one_asset(app: &AppHandle, asset_id: &str) -> AppResult<crate::domain::catalog_model::Asset> {
    let (mut assets, _) = collect_assets(app, std::slice::from_ref(&asset_id.to_string()))?;
    assets
        .pop()
        .ok_or_else(|| AppError::msg(format!("asset not found: {asset_id}")))
}
