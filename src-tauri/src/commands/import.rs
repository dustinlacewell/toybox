//! `merge_seed_entries`: the importer commit. The work (validate, upsert into
//! the manifest, rebuild the catalog) is `toybox_core::ops::import::merge_entries`.

use serde_json::Value;
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::infra::library;
use toybox_core::ops::browse::{summary, LibrarySummary};
use toybox_core::ops::import::merge_entries;

#[tauri::command]
pub async fn merge_seed_entries(app: AppHandle, library: String, entries: Vec<Value>) -> AppResult<LibrarySummary> {
    let handle = library::resolve(&app, &library)?;
    // Into a remote library the staged files are uploaded and the server commits.
    if let Some(remote) = library::remote_if(&app, &library)? {
        return crate::commands::remote::blocking(move || {
            let total = entries.len();
            let (imported, errors) = remote.import_rows(entries).map_err(AppError::from)?;
            if errors.is_empty() {
                return summary(&handle);
            }
            let shown: Vec<String> = errors.iter().take(5).map(|e| format!("{}: {}", e.id.as_deref().unwrap_or("an item"), e.message)).collect();
            let more = errors.len().saturating_sub(5);
            Err(AppError::msg(format!(
                "{} of {} items could not be imported ({} were added). {}{}",
                errors.len(), total, imported.len(), shown.join(" | "),
                if more > 0 { format!(" | and {more} more") } else { String::new() }
            )))
        })
        .await;
    }
    tauri::async_runtime::spawn_blocking(move || {
        merge_entries(&handle, entries)?;
        summary(&handle)
    })
    .await
    .map_err(|error| AppError::msg(format!("Import worker failed: {error}")))?
}
