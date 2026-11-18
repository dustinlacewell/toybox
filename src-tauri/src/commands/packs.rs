//! Compatibility source metadata API. The work is `toybox_core::ops::packs`.

use tauri::AppHandle;

use crate::domain::pack_meta::PackMeta;
use crate::error::AppResult;
use crate::infra::library;

#[tauri::command]
pub fn load_packs(app: AppHandle, library: String) -> AppResult<Vec<PackMeta>> {
    let handle=library::resolve(&app, &library)?;
    toybox_core::ops::packs::load_packs_for(&handle)
}
