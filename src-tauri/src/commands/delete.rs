//! `delete_assets`: remove items and their exclusively owned files from one
//! library. The work is `toybox_core::ops::delete::delete_from_library`.

use serde::Serialize;
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::infra::library;
use toybox_core::infra::store::Store;
use toybox_core::ops::browse::{summary, LibrarySummary};
use toybox_core::ops::delete::delete_from_library;

/// What a delete did: the ids that are gone, and where the library stands.
#[derive(Serialize)]
pub struct Deleted {
    pub deleted: Vec<String>,
    pub summary: LibrarySummary,
}

#[tauri::command]
pub async fn delete_assets(app: AppHandle, library: String, asset_ids: Vec<String>) -> AppResult<Deleted> {
    let handle = library::resolve(&app, &library)?;
    // On a remote library the server deletes; the local cache and files follow.
    let remote = library::remote_if(&app, &library)?;
    tauri::async_runtime::spawn_blocking(move || {
        match remote {
            Some(remote) => remote.delete(&asset_ids).map_err(AppError::from)?,
            None => {
                delete_from_library(&handle.root, &handle.cache_path, &asset_ids)?;
            }
        }
        let still_there = Store::for_handle(&handle).existing_asset_ids(&asset_ids)?;
        Ok(Deleted { deleted: asset_ids.into_iter().filter(|id| !still_there.contains(id)).collect(), summary: summary(&handle)? })
    })
    .await
    .map_err(|error| AppError::msg(format!("Delete worker failed: {error}")))?
}
