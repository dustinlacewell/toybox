//! `export_copy`: copy selected assets as self-contained loose filesets into a
//! target directory. The work is `toybox_core::ops::export_copy`.

use tauri::AppHandle;

use crate::domain::export_model::{ExportCopyReq, ExportReport};
use crate::error::AppResult;
use crate::infra::library;

#[tauri::command]
pub async fn export_copy(app: AppHandle, library: String, req: ExportCopyReq) -> AppResult<ExportReport> {
    let handle = library::resolve(&app, &library)?;
    // A remote library downloads what the export reads, so this runs off the async workers.
    crate::commands::remote::blocking(move || toybox_core::ops::export_copy::export_copy(&handle, &req)).await
}
