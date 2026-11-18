//! Thumbnail persistence. The frontend renders thumbnails with three.js and
//! hands the PNG bytes here; Rust owns where they land and records state in the
//! catalog. Resume after a crash is driven by which PNGs exist on disk. Each
//! library has its own thumbnail folder, so equal asset ids in two libraries
//! never share a picture.

use crate::domain::catalog_model::{ThumbMeta, ThumbState};
use crate::error::AppResult;
use crate::infra::registry::LibraryHandle;
use crate::infra::store::Store;
use crate::infra::{appdata, fsio};

/// Write a rendered thumbnail PNG. The PNG on disk is the source of truth for
/// "is this thumbnail done" (see `list_pending_thumbs`), so we deliberately do
/// NOT rewrite the catalog here — that would mean 1500+ full-file rewrites over
/// a generation run. Catalog thumb state is reconciled from disk on load.
pub fn save_thumb(handle: &LibraryHandle, asset_id: &str, png_bytes: &[u8]) -> AppResult<()> {
    let path = handle.thumb_path(asset_id);
    fsio::write_bytes_atomic(&path, png_bytes)?;
    fsio::remove_file_if_exists(&path.with_extension("stale"))?;
    // A remote library offers the picture to its server (quietly; only a write or admin token can).
    if let Some(link) = &handle.remote {
        link.files.share_thumb(asset_id, png_bytes);
    }
    Ok(())
}

/// A changed preview model affects this animation's cached picture only.
pub fn invalidate_thumb(handle: &LibraryHandle, asset_id: &str) -> AppResult<()> {
    let path = handle.thumb_path(asset_id);
    fsio::write_bytes(&path.with_extension("stale"), &[])
}

/// Missing PNGs and durable refresh markers form the work queue. Existing PNGs
/// remain readable throughout a refresh, including after an interrupted run.
/// The ids are streamed from the database against one listing of the thumbnail
/// folder: no catalog is built.
pub fn list_pending_thumbs(handle: &LibraryHandle) -> AppResult<Vec<String>> {
    super::catalog::open_library(handle)?;
    super::catalog::refresh_animation_preview_thumbs(handle)?;
    let store = Store::for_handle(handle);
    if !store.catalog_exists() {
        return Ok(Vec::new());
    }
    pending_ids_in(handle, &store)
}

/// The catalog assets whose picture is missing or marked stale, read from the store.
pub(crate) fn pending_ids_in(handle: &LibraryHandle, store: &Store) -> AppResult<Vec<String>> {
    let on_disk: std::collections::HashSet<String> = match std::fs::read_dir(&handle.thumbs_dir) {
        Ok(listing) => listing.flatten().filter_map(|entry| entry.file_name().into_string().ok()).collect(),
        Err(_) => std::collections::HashSet::new(),
    };
    let mut pending = Vec::new();
    store.for_each_asset_id(&mut |id| {
        let name = appdata::sanitize_id(id);
        let has = on_disk.contains(&format!("{name}.png")) || handle.remote.is_some() && handle.has_thumb(id);
        if !has || on_disk.contains(&format!("{name}.stale")) {
            pending.push(id.to_string());
        }
    })?;
    Ok(pending)
}

/// [`pending_ids_in`] for a catalog already in memory (the reference the tests compare it with).
#[cfg(test)]
pub(crate) fn pending_ids(handle: &LibraryHandle, catalog: &crate::domain::catalog_model::Catalog) -> Vec<String> {
    let mut pending = Vec::new();
    for asset in &catalog.assets {
        let path = handle.thumb_path(&asset.id);
        if !handle.has_thumb(&asset.id) || fsio::exists(&path.with_extension("stale")) {
            pending.push(asset.id.clone());
        }
    }
    pending
}

/// Invalidate selected/all thumbnails. Refresh mode preserves existing PNGs;
/// explicit cache clearing removes them. The catalog is untouched.
pub fn clear_thumbs(handle: &LibraryHandle, asset_ids: Option<Vec<String>>, preserve_existing: Option<bool>) -> AppResult<()> {
    let store = Store::for_handle(handle);
    if preserve_existing.unwrap_or(false) {
        super::catalog::open_library(handle)?;
        let mark = |id: &str| fsio::write_bytes(&handle.thumb_path(id).with_extension("stale"), &[]);
        match asset_ids {
            Some(ids) => {
                let known = store.existing_asset_ids(&ids)?;
                for id in ids.iter().filter(|id| known.contains(*id)) {
                    mark(id)?;
                }
            }
            None => {
                let mut failure = None;
                store.for_each_asset_id(&mut |id| {
                    if failure.is_none() {
                        failure = mark(id).err();
                    }
                })?;
                if let Some(error) = failure {
                    return Err(error);
                }
            }
        }
        return Ok(());
    }
    if let Some(ids) = asset_ids {
        // Resolve only catalog-owned thumbnails; avoid a catalog rewrite per item.
        super::catalog::open_library(handle)?;
        let known = store.existing_asset_ids(&ids)?;
        for id in ids.iter().filter(|id| known.contains(*id)) {
            fsio::remove_file_if_exists(&handle.thumb_path(id))?;
        }
        return Ok(());
    }
    if fsio::exists(&handle.thumbs_dir) {
        std::fs::remove_dir_all(&handle.thumbs_dir)?;
    }
    Ok(())
}

/// Update an asset's thumbnail state (e.g. mark `error` on a failed render).
/// Only that asset's personal row is written: no catalog is loaded or saved.
pub fn set_thumb_state(
    handle: &LibraryHandle,
    asset_id: &str,
    state: ThumbState,
    error: Option<String>,
) -> AppResult<()> {
    super::catalog::open_library(handle)?;
    Store::for_handle(handle).set_thumb(asset_id, &ThumbMeta { state, error })?;
    Ok(())
}
