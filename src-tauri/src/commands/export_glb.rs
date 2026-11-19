//! `export_glb`: pack each selected asset into a single self-contained `.glb`.
//! The work is `toybox_host::Host::export_glb`.

use tauri::AppHandle;

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::domain::export_model::{ExportGlbReq, ExportReport};
use crate::error::AppResult;

#[tauri::command]
pub async fn export_glb(app: AppHandle, library: String, req: ExportGlbReq) -> AppResult<ExportReport> {
    let host = host(&app);
    blocking(move || host.export_glb(&library, &req)).await
}
