//! `recenter_asset`: correct an asset's origin in place. The work is
//! `toybox_core::ops::recenter`.

use tauri::AppHandle;

use crate::domain::gltf_origin::{Aabb, Align, Axis};
use crate::error::AppResult;
use crate::infra::library;

#[tauri::command]
pub async fn recenter_asset(
    app: AppHandle,
    library: String,
    asset_id: String,
    axis: Axis,
    align: Align,
) -> AppResult<Aabb> {
    let handle = library::resolve(&app, &library)?;
    let aabb = toybox_core::ops::recenter::recenter_asset(&handle, &asset_id, axis, align)?;
    // The file changed under its storage key; core recomputed the fingerprint, the window learns it.
    crate::commands::fingerprint::push_asset_hash(&app, &handle, &asset_id);
    Ok(aabb)
}
