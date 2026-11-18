//! Persisted application settings at `<app-data>/settings.json`. The schema and
//! the path-based read/write live in `toybox-core`; these wrappers resolve the
//! path from the app handle.

use tauri::AppHandle;

use crate::error::AppResult;
use crate::infra::appdata;

pub use toybox_core::infra::settings::AppSettings;

/// Load settings, defaulting to empty whenever the file can't be read as our
/// schema (see `toybox_core::infra::settings::load_from`). This is
/// load-bearing: `load` runs in `.setup()`, where any `Err` would abort window
/// creation, so it must never fail.
pub fn load(app: &AppHandle) -> AppResult<AppSettings> {
    toybox_core::infra::settings::load_from(&appdata::settings_path(app)?)
}

pub fn save(app: &AppHandle, settings: &AppSettings) -> AppResult<()> {
    toybox_core::infra::settings::save_to(&appdata::settings_path(app)?, settings)
}
