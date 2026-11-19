//! `recenter_asset`: correct an asset's origin in place. The work is
//! `toybox_host::Host::recenter_asset`; the window is then told the new fingerprint.

use tauri::AppHandle;

use crate::app_host::host;
use crate::commands::fingerprint::push_hashes;
use crate::domain::gltf_origin::{Aabb, Align, Axis};
use crate::error::AppResult;

#[tauri::command]
pub async fn recenter_asset(
    app: AppHandle,
    library: String,
    asset_id: String,
    axis: Axis,
    align: Align,
) -> AppResult<Aabb> {
    let host = host(&app);
    let aabb = host.recenter_asset(&library, &asset_id, axis, align)?;
    // The file changed under its storage key; core recomputed the fingerprint, the window learns it.
    if let Some(hashes) = host.asset_hash_event(&library, &asset_id) {
        push_hashes(&app, hashes);
    }
    Ok(aabb)
}
