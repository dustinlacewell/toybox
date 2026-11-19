//! `merge_seed_entries`: the importer commit. The work (validate, upsert into
//! the manifest, rebuild the catalog; or upload to a remote library's server) is
//! `toybox_host::Host::merge_seed_entries`.

use serde_json::Value;
use tauri::AppHandle;
use toybox_core::ops::browse::LibrarySummary;

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::error::AppResult;

#[tauri::command]
pub async fn merge_seed_entries(app: AppHandle, library: String, entries: Vec<Value>) -> AppResult<LibrarySummary> {
    let host = host(&app);
    blocking(move || host.merge_seed_entries(&library, entries)).await
}
