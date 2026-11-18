//! Preview source reads avoid deserializing the entire catalog per thumbnail.
//! The reads and the containment check are `toybox_core::ops::preview_source`.
use crate::{
    error::{AppError, AppResult},
    infra::library,
};
use serde_json::Value;
use tauri::AppHandle;
use toybox_core::ops::preview_source::{read_pair, read_source};

#[tauri::command]
pub async fn read_preview_source(
    app: AppHandle,
    library: String,
    relative_path: String,
    clip_index: Option<usize>,
) -> AppResult<Value> {
    let handle = library::resolve(&app, &library)?;
    tauri::async_runtime::spawn_blocking(move || {
        handle.ensure_gltf(&relative_path)?;
        read_source(&handle.root, &relative_path, clip_index)
    })
        .await
        .map_err(|error| AppError::msg(format!("Preview worker failed: {error}")))?
}

#[tauri::command]
pub async fn read_preview_pair(
    app: AppHandle,
    library: String,
    animation_path: String,
    clip_index: usize,
    model_path: String,
) -> AppResult<Value> {
    let handle = library::resolve(&app, &library)?;
    tauri::async_runtime::spawn_blocking(move || {
        handle.ensure_gltf(&animation_path)?;
        handle.ensure_gltf(&model_path)?;
        read_pair(&handle.root, &animation_path, clip_index, &model_path)
    })
    .await
    .map_err(|error| AppError::msg(format!("Preview worker failed: {error}")))?
}
