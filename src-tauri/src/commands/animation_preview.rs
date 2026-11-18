use crate::{error::AppResult, infra::library};
use serde_json::Value;
use tauri::AppHandle;
use toybox_core::ops::animation_preview::{self, PreviewChoice};

#[tauri::command]
pub async fn compatible_previews(
    app: AppHandle,
    library: String,
    asset_id: String,
) -> AppResult<Vec<PreviewChoice>> {
    let handle = library::resolve(&app, &library)?;
    animation_preview::compatible_previews(&handle, &asset_id)
}

#[tauri::command]
pub async fn read_animation_preview(
    app: AppHandle,
    library: String,
    animation_id: String,
    model_id: String,
) -> AppResult<Value> {
    let handle = library::resolve(&app, &library)?;
    crate::commands::remote::blocking(move || animation_preview::read_animation_preview(&handle, &animation_id, &model_id)).await
}
