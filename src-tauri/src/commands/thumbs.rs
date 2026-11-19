//! Thumbnail persistence commands. The frontend renders thumbnails with three.js
//! and hands the PNG bytes here; the work is `toybox_host::Host`.

use tauri::AppHandle;

use crate::app_host::host;
use crate::domain::catalog_model::ThumbState;
use crate::error::AppResult;

#[tauri::command]
pub async fn save_thumb(app: AppHandle, library: String, asset_id: String, png_bytes: Vec<u8>) -> AppResult<()> {
    host(&app).save_thumb(&library, &asset_id, &png_bytes)
}

#[tauri::command]
pub async fn invalidate_thumb(app: AppHandle, library: String, asset_id: String) -> AppResult<()> {
    host(&app).invalidate_thumb(&library, &asset_id)
}

#[tauri::command]
pub async fn set_thumb_state(
    app: AppHandle,
    library: String,
    asset_id: String,
    state: ThumbState,
    error: Option<String>,
) -> AppResult<()> {
    host(&app).set_thumb_state(&library, &asset_id, state, error)
}

#[tauri::command]
pub async fn clear_thumbs(app: AppHandle, library: String, asset_ids: Option<Vec<String>>, preserve_existing: Option<bool>) -> AppResult<()> {
    host(&app).clear_thumbs(&library, asset_ids, preserve_existing)
}
