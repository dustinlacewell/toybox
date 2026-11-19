//! Preview source reads avoid deserializing the entire catalog per thumbnail.
//! The work is `toybox_host::Host::read_preview_source` / `read_preview_pair`.
use serde_json::Value;
use tauri::AppHandle;

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::error::AppResult;

#[tauri::command]
pub async fn read_preview_source(
    app: AppHandle,
    library: String,
    relative_path: String,
    clip_index: Option<usize>,
) -> AppResult<Value> {
    let host = host(&app);
    blocking(move || host.read_preview_source(&library, &relative_path, clip_index)).await
}

#[tauri::command]
pub async fn read_preview_pair(
    app: AppHandle,
    library: String,
    animation_path: String,
    clip_index: usize,
    model_path: String,
) -> AppResult<Value> {
    let host = host(&app);
    blocking(move || host.read_preview_pair(&library, &animation_path, clip_index, &model_path)).await
}
