//! `export_glb`: pack each selected asset into a single self-contained `.glb`.
//! The work is `toybox_core::ops::export_glb`.

use tauri::AppHandle;

use crate::domain::export_model::{ExportGlbReq, ExportReport};
use crate::error::AppResult;
use crate::infra::library;

#[tauri::command]
pub async fn export_glb(app: AppHandle, library: String, req: ExportGlbReq) -> AppResult<ExportReport> {
    let handle = library::resolve(&app, &library)?;
    crate::commands::remote::blocking(move || toybox_core::ops::export_glb::export_glb(&handle, &req)).await
}
