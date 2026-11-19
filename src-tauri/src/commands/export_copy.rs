//! `export_copy`: copy selected assets as self-contained loose filesets into a
//! target directory. The work is `toybox_host::Host::export_copy`.

use tauri::AppHandle;

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::domain::export_model::{ExportCopyReq, ExportReport};
use crate::error::AppResult;

#[tauri::command]
pub async fn export_copy(app: AppHandle, library: String, req: ExportCopyReq) -> AppResult<ExportReport> {
    let host = host(&app);
    // A remote library downloads what the export reads, so this runs off the async workers.
    blocking(move || host.export_copy(&library, &req)).await
}
