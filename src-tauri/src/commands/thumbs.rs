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
    fsio::write_bytes_atomic(&path, &png_bytes)?;
    fsio::remove_file_if_exists(&path.with_extension("stale"))
}

/// A changed preview model affects this animation's cached picture only.
#[tauri::command]
pub async fn invalidate_thumb(app:AppHandle,asset_id:String)->AppResult<()> {
    let path=appdata::thumb_path(&app,&asset_id)?;
    fsio::write_bytes(&path.with_extension("stale"), &[])
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

/// Missing PNGs and durable refresh markers form the work queue. Existing PNGs
/// remain readable throughout a refresh, including after an interrupted run.
#[tauri::command]
pub async fn list_pending_thumbs(app: AppHandle) -> AppResult<Vec<String>> {
    let Some(catalog) = load_catalog_inner(&app)? else {
        return Ok(Vec::new());
    };
    let mut pending = Vec::new();
    for asset in &catalog.assets {
        let path = appdata::thumb_path(&app, &asset.id)?;
        if !fsio::exists(&path) || fsio::exists(&path.with_extension("stale")) {
            pending.push(asset.id.clone());
        }
    }
    Ok(pending)
}

/// Invalidate selected/all thumbnails. Refresh mode preserves existing PNGs;
/// explicit cache clearing removes them. The catalog is untouched.
#[tauri::command]
pub async fn clear_thumbs(app: AppHandle, asset_ids: Option<Vec<String>>, preserve_existing: Option<bool>) -> AppResult<()> {
    if preserve_existing.unwrap_or(false) {
        if let Some(catalog) = load_catalog_inner(&app)? {
            let ids = asset_ids.map(|ids| ids.into_iter().collect::<std::collections::HashSet<_>>());
            for asset in &catalog.assets {
                if ids.as_ref().map_or(true, |ids| ids.contains(&asset.id)) {
                    fsio::write_bytes(&appdata::thumb_path(&app, &asset.id)?.with_extension("stale"), &[])?;
                }
            }
        }
        return Ok(());
    }
    if let Some(ids) = asset_ids {
        // Resolve only catalog-owned thumbnails; avoid a catalog rewrite per item.
        if let Some(catalog) = load_catalog_inner(&app)? {
            let ids: std::collections::HashSet<_> = ids.into_iter().collect();
            for asset in catalog.assets.iter().filter(|asset| ids.contains(&asset.id)) {
                fsio::remove_file_if_exists(&appdata::thumb_path(&app, &asset.id)?)?;
            }
        }
        return Ok(());
    }
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
