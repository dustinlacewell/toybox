//! Persisted application settings at `<app-data>/settings.json`. Currently holds
//! only the user-chosen library root; this is the durable backing for the
//! runtime `LibraryState` so the choice survives restarts.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::error::AppResult;
use crate::infra::{appdata, fsio};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Absolute path to the asset library the user picked, if any.
    pub library_root: Option<String>,
    /// Absolute path to the user's FBX2glTF executable, if configured. Used by
    /// the importer to convert `.fbx` sources; absent means FBX import is off.
    #[serde(default)]
    pub fbx2gltf_path: Option<String>,
}

/// Load settings, defaulting to empty whenever the file can't be read as our
/// schema — absent, unreadable (locked / ACL-denied), or corrupt all collapse to
/// "no settings yet". This is load-bearing: `load` runs in `.setup()`, where any
/// `Err` would abort window creation, so it must never fail.
pub fn load(app: &AppHandle) -> AppResult<AppSettings> {
    let path = appdata::settings_path(app)?;
    let Ok(text) = fsio::read_text(&path) else {
        return Ok(AppSettings::default());
    };
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

pub fn save(app: &AppHandle, settings: &AppSettings) -> AppResult<()> {
    let path = appdata::settings_path(app)?;
    let text = serde_json::to_string_pretty(settings)?;
    fsio::write_text(&path, &text)
}
