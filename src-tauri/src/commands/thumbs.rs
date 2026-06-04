//! Thumbnail persistence. The frontend renders thumbnails with three.js and
//! hands the PNG bytes here; Rust owns where they land and records state in the
//! catalog. Resume after a crash is driven by which PNGs exist on disk.

use tauri::AppHandle;

use crate::domain::catalog_model::ThumbState;
use crate::error::AppResult;
use crate::infra::{appdata, fsio};

use super::catalog::{load_catalog_inner, save_catalog_inner};

/// Write a rendered thumbnail PNG. The PNG on disk is the source of truth for
/// "is this thumbnail done" (see `list_pending_thumbs`), so we deliberately do
/// NOT rewrite the catalog here — that would mean 1500+ full-file rewrites over
/// a generation run. Catalog thumb state is reconciled from disk on load.
#[tauri::command]
pub async fn save_thumb(app: AppHandle, asset_id: String, png_bytes: Vec<u8>) -> AppResult<()> {
    let path = appdata::thumb_path(&app, &asset_id)?;
    fsio::write_bytes(&path, &png_bytes)
}

/// Update an asset's thumbnail state (e.g. mark `error` on a failed render).
#[tauri::command]
pub async fn set_thumb_state(
    app: AppHandle,
    asset_id: String,
    state: ThumbState,
    error: Option<String>,
) -> AppResult<()> {
    set_thumb_state_inner(&app, &asset_id, state, error)
}

/// Return the ids of assets whose thumbnail PNG does not yet exist on disk —
/// the work queue. Disk is the source of truth so this is correct across
/// restarts regardless of catalog state.
#[tauri::command]
pub async fn list_pending_thumbs(app: AppHandle) -> AppResult<Vec<String>> {
    let Some(catalog) = load_catalog_inner(&app)? else {
        return Ok(Vec::new());
    };
    let mut pending = Vec::new();
    for asset in &catalog.assets {
        let path = appdata::thumb_path(&app, &asset.id)?;
        if !fsio::exists(&path) {
            pending.push(asset.id.clone());
        }
    }
    Ok(pending)
}

/// Delete all cached thumbnail PNGs (used by "regenerate all"). The catalog is
/// untouched; thumb states reconcile to `missing` on next load.
#[tauri::command]
pub async fn clear_thumbs(app: AppHandle) -> AppResult<()> {
    let dir = appdata::thumb_dir(&app)?;
    if fsio::exists(&dir) {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

fn set_thumb_state_inner(
    app: &AppHandle,
    asset_id: &str,
    state: ThumbState,
    error: Option<String>,
) -> AppResult<()> {
    let Some(mut catalog) = load_catalog_inner(app)? else {
        return Ok(());
    };
    if let Some(asset) = catalog.assets.iter_mut().find(|a| a.id == asset_id) {
        asset.thumb.state = state;
        asset.thumb.error = error;
        save_catalog_inner(app, &catalog)?;
    }
    Ok(())
}
