//! `scan_library`: the explicit rescan. It rebuilds a library's catalog from its manifest (or, for a
//! remote library, downloads it again) and says where the library stands; the window then queries it.
//! The work is `toybox_core::ops::scan`.

use tauri::AppHandle;

use crate::commands::remote::blocking;
use crate::error::{AppError, AppResult};
use crate::infra::library;
use toybox_core::ops::browse::{summary, LibrarySummary};
use toybox_core::ops::scan::scan_library_catalog;

#[tauri::command]
pub async fn scan_library(app: AppHandle, library: String, force_reseed: bool) -> AppResult<LibrarySummary> {
    let handle = library::resolve(&app, &library)?;
    // Rescanning a remote library downloads its catalog again.
    if let Some(remote) = library::remote_if(&app, &library)? {
        return blocking(move || {
            remote.refresh(true).map_err(AppError::from)?;
            summary(&handle)
        })
        .await;
    }
    let _ = force_reseed;
    blocking(move || {
        scan_library_catalog(&handle)?;
        summary(&handle)
    })
    .await
}
