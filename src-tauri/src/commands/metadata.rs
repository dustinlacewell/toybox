//! `update_asset_metadata`: change the library tags and the provenance of items.
//! The work is `toybox_host::Host::update_asset_metadata`; on a remote library
//! the server makes the change (`POST /assets/metadata`) and this computer's cache
//! follows.

use tauri::AppHandle;
use toybox_core::ops::metadata::MetadataEdit;

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::domain::catalog_model::Asset;
use crate::error::AppResult;

/// Returns the top-level items the edit touched (an edited variant comes inside its
/// parent), as the library's cache now holds them, personal metadata included.
#[tauri::command]
pub async fn update_asset_metadata(app: AppHandle, library: String, edits: Vec<MetadataEdit>) -> AppResult<Vec<Asset>> {
    let host = host(&app);
    blocking(move || host.update_asset_metadata(&library, &edits)).await
}
