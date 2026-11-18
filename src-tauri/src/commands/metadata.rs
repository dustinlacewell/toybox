//! `update_asset_metadata`: change the library tags and the provenance of items.
//! The work is `toybox_core::ops::metadata::update_entries`; on a remote library
//! the server makes the change (`POST /assets/metadata`) and this computer's cache
//! follows.

use tauri::AppHandle;

use crate::domain::catalog_model::Asset;
use crate::error::{AppError, AppResult};
use crate::infra::library;
use toybox_core::ops::metadata::{update_entries, MetadataEdit};

/// Returns the top-level items the edit touched (an edited variant comes inside its
/// parent), as the library's cache now holds them, personal metadata included.
#[tauri::command]
pub async fn update_asset_metadata(app: AppHandle, library: String, edits: Vec<MetadataEdit>) -> AppResult<Vec<Asset>> {
    if let Some(remote) = library::remote_if(&app, &library)? {
        return crate::commands::remote::blocking(move || remote.update_metadata(&edits).map_err(AppError::from)).await;
    }
    let handle = library::resolve(&app, &library)?;
    tauri::async_runtime::spawn_blocking(move || {
        // Only the edited items are read and written; they come back as they are now.
        update_entries(&handle, &edits)
    })
    .await
    .map_err(|error| AppError::msg(format!("Metadata worker failed: {error}")))?
}
