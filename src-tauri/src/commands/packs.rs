//! Compatibility source metadata API. The work is `toybox_host::Host::load_packs`.

use tauri::AppHandle;

use crate::app_host::host;
use crate::domain::pack_meta::PackMeta;
use crate::error::AppResult;

#[tauri::command]
pub fn load_packs(app: AppHandle, library: String) -> AppResult<Vec<PackMeta>> {
    host(&app).load_packs(&library)
}
