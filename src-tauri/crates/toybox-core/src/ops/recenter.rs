//! `recenter_asset`: correct an asset's origin in place. Reads the asset's glTF,
//! bakes a root-node translation that lands the chosen bounding-box point
//! (min/center/max) on the local origin for one axis, and rewrites the `.gltf`.
//! The `.bin` is never touched (the fix is a node transform, correct for skinned
//! and static meshes alike). The cached thumbnail is dropped so it regenerates.

use crate::domain::gltf_origin::{recenter, Aabb, Align, Axis};
use crate::domain::{gltf_parse, paths};
use crate::error::{AppError, AppResult};
use crate::infra::fsio;
use crate::infra::registry::LibraryHandle;

use crate::infra::store::Store;

/// Apply an origin correction to one asset and persist it to the source `.gltf`.
/// Returns the resulting world-space AABB so the viewer can re-frame.
pub fn recenter_asset(
    handle: &LibraryHandle,
    asset_id: &str,
    axis: Axis,
    align: Align,
) -> AppResult<Aabb> {
    if handle.is_remote() {
        return Err(AppError::msg(crate::infra::registry::REMOTE_NO_FILE_EDITS));
    }
    super::catalog::open_library(handle)?;
    let store = Store::for_handle(handle);
    if !store.catalog_exists() {
        return Err(AppError::msg("no catalog; scan first"));
    }
    let asset = store
        .load_top_assets(&[asset_id.to_string()])?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::msg(format!("asset not found: {asset_id}")))?;
    let asset = &asset;

    let gltf_abs = paths::abs_under_root(&handle.root_str(), &asset.fileset()?.gltf);

    let text = fsio::read_text(&gltf_abs)?;
    let doc = gltf_parse::parse(&text)?;

    let (corrected, aabb) = recenter(&doc, axis, align)?;

    let out_text = serde_json::to_string(&corrected)?;
    fsio::write_text(&gltf_abs, &out_text)?;

    // The geometry moved relative to the camera/thumb framing — drop the cached
    // thumbnail so it regenerates on the next pass.
    let thumb = handle.thumb_path(asset_id);
    fsio::remove_file_if_exists(&thumb)?;

    // The one edit in place: the stored file changed under an unchanged storage key,
    // so the item's content fingerprint is recomputed.
    super::content_hash::rehash(handle, &[asset]);

    Ok(aabb)
}
