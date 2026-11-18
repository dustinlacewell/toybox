//! `transfer_assets`: copy or move items from one library into another. The work
//! is `toybox_core::ops::transfer::transfer_between`; this wrapper resolves the
//! two libraries and reports progress to the window.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, AppResult};
use crate::infra::library;
use toybox_core::ops::transfer::{OnConflict, TransferMode, TransferOptions, TransferProgress, TransferReport};
use toybox_remote::transfer::{transfer, Side};

/// Event name progress is emitted under (payload: `TransferProgressEvent`).
pub const PROGRESS_EVENT: &str = "transfer-progress";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferProgressEvent {
    from_library: String,
    to_library: String,
    #[serde(flatten)]
    progress: TransferProgress,
}

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
    // Either end may be on this computer or on a server; the transfer code handles every pairing.
    let source_remote = library::remote_if(&app, &from_library)?;
    let target_remote = library::remote_if(&app, &to_library)?;
    let source = if source_remote.is_none() { Some(library::resolve(&app, &from_library)?) } else { None };
    let target = if target_remote.is_none() { Some(library::resolve(&app, &to_library)?) } else { None };
    let options = TransferOptions { mode, on_conflict, include_clips: include_clips.unwrap_or(false) };
    let events = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let from = match (&source_remote, &source) {
            (Some(remote), _) => Side::Remote(remote),
            (None, Some(handle)) => Side::Local(handle),
            (None, None) => return Err(AppError::msg("Unknown source library")),
        };
        let to = match (&target_remote, &target) {
            (Some(remote), _) => Side::Remote(remote),
            (None, Some(handle)) => Side::Local(handle),
            (None, None) => return Err(AppError::msg("Unknown target library")),
        };
        transfer(from, to, &ids, options, &|progress| {
            let _ = events.emit(PROGRESS_EVENT, TransferProgressEvent { from_library: from_library.clone(), to_library: to_library.clone(), progress });
        })
    })
    .await
    .map_err(|error| AppError::msg(format!("Transfer worker failed: {error}")))?
}
