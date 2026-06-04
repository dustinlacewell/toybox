//! `recenter_asset`: correct an asset's origin in place. Reads the asset's glTF,
//! bakes a root-node translation that lands the chosen bounding-box point
//! (min/center/max) on the local origin for one axis, and rewrites the `.gltf`.
//! The `.bin` is never touched (the fix is a node transform, correct for skinned
//! and static meshes alike). The cached thumbnail is dropped so it regenerates.

use tauri::AppHandle;

use crate::config;
use crate::domain::gltf_origin::{recenter, Aabb, Align, Axis};
use crate::domain::{gltf_parse, paths};
use crate::error::{AppError, AppResult};
use crate::infra::{appdata, fsio};

use super::catalog::load_catalog_inner;

/// Apply an origin correction to one asset and persist it to the source `.gltf`.
/// Returns the resulting world-space AABB so the viewer can re-frame.
#[tauri::command]
pub async fn recenter_asset(
    app: AppHandle,
    asset_id: String,
    axis: Axis,
    align: Align,
) -> AppResult<Aabb> {
    let catalog =
        load_catalog_inner(&app)?.ok_or_else(|| AppError::msg("no catalog; scan first"))?;
    let asset = catalog
        .assets
        .iter()
        .find(|a| a.id == asset_id)
        .ok_or_else(|| AppError::msg(format!("asset not found: {asset_id}")))?;

    let root = config::LIBRARY_ROOT;
    let gltf_abs = paths::abs_under_root(root, &asset.fileset.gltf);

    let text = fsio::read_text(&gltf_abs)?;
    let doc = gltf_parse::parse(&text)?;

    let (corrected, aabb) = recenter(&doc, axis, align)?;

    let out_text = serde_json::to_string(&corrected)?;
    fsio::write_text(&gltf_abs, &out_text)?;

    // The geometry moved relative to the camera/thumb framing — drop the cached
    // thumbnail so it regenerates on the next pass.
    let thumb = appdata::thumb_path(&app, &asset_id)?;
    if fsio::exists(&thumb) {
        std::fs::remove_file(&thumb)?;
    }

    Ok(aabb)
}
