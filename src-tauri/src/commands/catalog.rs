//! Path-resolution commands. The frontend turns the
//! absolute paths returned here into webview URLs via `convertFileSrc`. Every
//! command works on one library, named by its id.

use tauri::AppHandle;

use crate::domain::paths;
use crate::commands::remote::blocking;
use crate::error::AppResult;
use crate::infra::library;

#[tauri::command]
pub fn library_root(app: AppHandle, library: String) -> AppResult<String> {
    Ok(library::resolve(&app, &library)?.root_str())
}

/// Map a library-relative path to an absolute path the asset protocol can serve.
/// For a remote library the file (and the rest of the item it belongs to) is
/// downloaded first, off the main thread; the viewer shows its loading state
/// until this answers.
#[tauri::command]
pub async fn resolve_asset_path(app: AppHandle, library: String, rel_path: String) -> AppResult<String> {
    let handle = library::resolve(&app, &library)?;
    let resolve = move || -> AppResult<String> {
        handle.ensure_files(std::slice::from_ref(&rel_path))?;
        Ok(paths::abs_under_root(&handle.root_str(), &rel_path).to_string_lossy().to_string())
    };
    blocking(resolve).await
}

#[tauri::command]
pub fn thumb_dir(app: AppHandle, library: String) -> AppResult<String> {
    Ok(library::resolve(&app, &library)?.thumbs_dir.to_string_lossy().to_string())
}

/// Where an item's thumbnail is. A remote library's shared picture is downloaded
/// the first time it is asked for; if that fails the path is still returned and
/// the picture simply does not show.
#[tauri::command]
pub async fn thumb_path(app: AppHandle, library: String, asset_id: String) -> AppResult<String> {
    let handle = library::resolve(&app, &library)?;
    let path = handle.thumb_path(&asset_id).to_string_lossy().to_string();
    if let Some(link) = handle.remote.clone() {
        let _ = blocking(move || Ok(link.files.ensure_thumb(&asset_id)?)).await;
    }
    Ok(path)
}
