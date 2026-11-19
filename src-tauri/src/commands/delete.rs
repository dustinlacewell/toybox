//! `delete_assets`: remove items and their exclusively owned files from one
//! library. The work is `toybox_host::Host::delete_assets`.

use tauri::AppHandle;
use toybox_host::ops::Deleted;

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::error::AppResult;

#[tauri::command]
pub async fn delete_assets(app: AppHandle, library: String, asset_ids: Vec<String>) -> AppResult<Deleted> {
    let host = host(&app);
    blocking(move || host.delete_assets(&library, asset_ids)).await
}
