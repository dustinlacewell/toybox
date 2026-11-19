//! Path-resolution commands. The frontend turns the
//! absolute paths returned here into webview URLs via `convertFileSrc`. Every
//! command works on one library, named by its id. The work is `toybox_host::Host`.

use tauri::AppHandle;

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::error::AppResult;

#[tauri::command]
pub fn library_root(app: AppHandle, library: String) -> AppResult<String> {
    host(&app).library_root(&library)
}

/// Map a library-relative path to an absolute path the asset protocol can serve.
/// For a remote library the file (and the rest of the item it belongs to) is
/// downloaded first, off the main thread; the viewer shows its loading state
/// until this answers.
#[tauri::command]
pub async fn resolve_asset_path(app: AppHandle, library: String, rel_path: String) -> AppResult<String> {
    let host = host(&app);
    blocking(move || host.resolve_asset_path(&library, &rel_path)).await
}

#[tauri::command]
pub fn thumb_dir(app: AppHandle, library: String) -> AppResult<String> {
    host(&app).thumb_dir(&library)
}

/// Where an item's thumbnail is. A remote library's shared picture is downloaded
/// the first time it is asked for; if that fails the path is still returned and
/// the picture simply does not show.
#[tauri::command]
pub async fn thumb_path(app: AppHandle, library: String, asset_id: String) -> AppResult<String> {
    let host = host(&app);
    blocking(move || host.thumb_path(&library, &asset_id)).await
}
