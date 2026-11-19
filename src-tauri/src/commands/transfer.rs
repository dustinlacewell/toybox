//! `transfer_assets`: copy or move items from one library into another. The work
//! is `toybox_host::Host::transfer_assets`; this wrapper reports progress to the window.

use tauri::{AppHandle, Emitter};
use toybox_core::ops::transfer::{OnConflict, TransferMode, TransferReport};

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::error::AppResult;

/// Event name progress is emitted under (payload: `TransferProgressEvent`).
pub const PROGRESS_EVENT: &str = "transfer-progress";

/// `ids` are raw ids of `from_library`. Items already in `to_library` are
/// skipped unless `on_conflict` is `replace`; `include_clips` also takes the
/// animation clips extracted from the selected models. A `move` deletes from
/// the source only what was copied.
#[tauri::command]
pub async fn transfer_assets(
    app: AppHandle,
    from_library: String,
    to_library: String,
    ids: Vec<String>,
    mode: TransferMode,
    on_conflict: OnConflict,
    include_clips: Option<bool>,
) -> AppResult<TransferReport> {
    let host = host(&app);
    let events = app.clone();
    blocking(move || {
        host.transfer_assets(&from_library, &to_library, &ids, mode, on_conflict, include_clips.unwrap_or(false), &|event| {
            let _ = events.emit(PROGRESS_EVENT, event);
        })
    })
    .await
}
