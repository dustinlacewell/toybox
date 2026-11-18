//! Thumbnail persistence commands. The frontend renders thumbnails with three.js
//! and hands the PNG bytes here; the work is `toybox_core::ops::thumbs`.

use tauri::AppHandle;

use crate::domain::catalog_model::ThumbState;
use crate::error::AppResult;
use crate::infra::library;
use toybox_core::ops::thumbs;

#[tauri::command]
pub async fn save_thumb(app: AppHandle, library: String, asset_id: String, png_bytes: Vec<u8>) -> AppResult<()> {
    thumbs::save_thumb(&library::resolve(&app, &library)?, &asset_id, &png_bytes)
}

#[tauri::command]
pub async fn invalidate_thumb(app:AppHandle,library: String,asset_id:String)->AppResult<()> {
    thumbs::invalidate_thumb(&library::resolve(&app, &library)?, &asset_id)
}

#[tauri::command]
pub async fn set_thumb_state(
    app: AppHandle,
    library: String,
    asset_id: String,
    state: ThumbState,
    error: Option<String>,
) -> AppResult<()> {
    thumbs::set_thumb_state(&library::resolve(&app, &library)?, &asset_id, state, error)
}

#[tauri::command]
pub async fn clear_thumbs(app: AppHandle, library: String, asset_ids: Option<Vec<String>>, preserve_existing: Option<bool>) -> AppResult<()> {
    thumbs::clear_thumbs(&library::resolve(&app, &library)?, asset_ids, preserve_existing)
}
