use serde_json::Value;
use tauri::AppHandle;
use toybox_core::ops::animation_preview::PreviewChoice;

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::error::AppResult;

#[tauri::command]
pub async fn compatible_previews(
    app: AppHandle,
    library: String,
    asset_id: String,
) -> AppResult<Vec<PreviewChoice>> {
    host(&app).compatible_previews(&library, &asset_id)
}

#[tauri::command]
pub async fn read_animation_preview(
    app: AppHandle,
    library: String,
    animation_id: String,
    model_id: String,
) -> AppResult<Value> {
    let host = host(&app);
    blocking(move || host.read_animation_preview(&library, &animation_id, &model_id)).await
}
