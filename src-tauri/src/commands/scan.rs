//! `scan_library`: the explicit rescan. It rebuilds a library's catalog from its manifest (or, for a
//! remote library, downloads it again) and says where the library stands; the window then queries it.
//! The work is `toybox_host::Host::scan_library`.

use tauri::AppHandle;
use toybox_core::ops::browse::LibrarySummary;

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::error::AppResult;

#[tauri::command]
pub async fn scan_library(app: AppHandle, library: String, force_reseed: bool) -> AppResult<LibrarySummary> {
    let _ = force_reseed;
    let host = host(&app);
    blocking(move || host.scan_library(&library)).await
}
