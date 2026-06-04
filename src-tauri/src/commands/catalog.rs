//! Catalog persistence and path-resolution commands. The frontend turns the
//! absolute paths returned here into webview URLs via `convertFileSrc`.

use tauri::AppHandle;

use crate::config;
use crate::domain::catalog_model::{Catalog, ThumbState};
use crate::domain::paths;
use crate::error::AppResult;
use crate::infra::{appdata, fsio};

#[tauri::command]
pub async fn load_catalog(app: AppHandle) -> AppResult<Option<Catalog>> {
    load_catalog_inner(&app)
}

#[tauri::command]
pub async fn save_catalog(app: AppHandle, catalog: Catalog) -> AppResult<()> {
    save_catalog_inner(&app, &catalog)
}

#[tauri::command]
pub fn library_root() -> String {
    config::LIBRARY_ROOT.to_string()
}

/// Map a library-relative path to an absolute path the asset protocol can serve.
#[tauri::command]
pub fn resolve_asset_path(rel_path: String) -> String {
    paths::abs_under_root(config::LIBRARY_ROOT, &rel_path)
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
pub fn thumb_dir(app: AppHandle) -> AppResult<String> {
    Ok(appdata::thumb_dir(&app)?.to_string_lossy().to_string())
}

#[tauri::command]
pub fn thumb_path(app: AppHandle, asset_id: String) -> AppResult<String> {
    Ok(appdata::thumb_path(&app, &asset_id)?
        .to_string_lossy()
        .to_string())
}

// --- inner helpers (no #[command]) so other command modules can reuse them ---

pub fn load_catalog_inner(app: &AppHandle) -> AppResult<Option<Catalog>> {
    let path = appdata::catalog_path(app)?;
    if !fsio::exists(&path) {
        return Ok(None);
    }
    let text = fsio::read_text(&path)?;
    let mut catalog: Catalog = serde_json::from_str(&text)?;
    reconcile_thumb_states(app, &mut catalog)?;
    Ok(Some(catalog))
}

/// The thumbnail PNG on disk is the source of truth for "is this thumb ready".
/// We never churn the catalog file with per-thumbnail writes; instead, every
/// time the catalog is handed to the frontend we derive `ready`/`missing` from
/// disk. An `error` recorded on disk-less assets is preserved.
fn reconcile_thumb_states(app: &AppHandle, catalog: &mut Catalog) -> AppResult<()> {
    for asset in &mut catalog.assets {
        let exists = fsio::exists(&appdata::thumb_path(app, &asset.id)?);
        asset.thumb.state = match (exists, &asset.thumb.state) {
            (true, _) => ThumbState::Ready,
            (false, ThumbState::Error) => ThumbState::Error,
            (false, _) => ThumbState::Missing,
        };
    }
    Ok(())
}

pub fn save_catalog_inner(app: &AppHandle, catalog: &Catalog) -> AppResult<()> {
    let path = appdata::catalog_path(app)?;
    let text = serde_json::to_string_pretty(catalog)?;
    fsio::write_text(&path, &text)
}
