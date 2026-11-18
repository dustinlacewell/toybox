//! Persisted application settings at `<app-data>/settings.json` (path given by
//! the host, see `infra::appdata::settings_path`): the library
//! registry (see `infra::registry`), plugin switches and settings, saved
//! exports and the converter path.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::infra::fsio;
use crate::infra::registry::LibraryEntry;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub animation_preview_version: u32,
    #[serde(default)]
    pub saved_exports: Vec<serde_json::Value>,
    /// The libraries the user has added, in the order they were added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<LibraryEntry>,
    /// Id of the library imports go to unless the user picks another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_library: Option<String>,
    /// The single library root of versions before several libraries. Read once
    /// to migrate it into `libraries`; never written.
    #[serde(default, skip_serializing)]
    pub library_root: Option<String>,
    /// Absolute path to the user's FBX2glTF executable, if configured. Used by
    /// the importer to convert `.fbx` sources; absent means FBX import is off.
    #[serde(default)]
    pub fbx2gltf_path: Option<String>,
    /// Ids of installed plugins the user has turned off. A plugin not listed
    /// is on, so newly installed and bundled plugins work without a setting.
    #[serde(default)]
    pub disabled_plugins: Vec<String>,
    /// What the user chose for the settings plugins declare, by plugin id.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub plugin_settings: std::collections::BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
}

impl AppSettings {
    /// Store one setting of a plugin; `null` or blank text removes it, so the
    /// plugin's declared default applies again.
    pub fn set_plugin_setting(&mut self, plugin_id: &str, key: &str, value: serde_json::Value) {
        let blank = value.is_null() || value.as_str().is_some_and(|text| text.trim().is_empty());
        let values = self.plugin_settings.entry(plugin_id.to_string()).or_default();
        if blank { values.remove(key); } else { values.insert(key.to_string(), value); }
        if values.is_empty() { self.plugin_settings.remove(plugin_id); }
    }
}

/// Load settings, defaulting to empty whenever the file can't be read as our
/// schema — absent, unreadable (locked / ACL-denied), or corrupt all collapse to
/// "no settings yet". This is load-bearing: the desktop app loads settings in
/// `.setup()`, where any `Err` would abort window creation, so reading must
/// never fail.
pub fn load_from(path: &Path) -> AppResult<AppSettings> {
    let Ok(text) = fsio::read_text(path) else {
        return Ok(AppSettings::default());
    };
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

pub fn save_to(path: &Path, settings: &AppSettings) -> AppResult<()> {
    let text = serde_json::to_string_pretty(settings)?;
    fsio::write_bytes_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plugin_settings_are_kept_per_plugin_and_blank_values_return_to_the_default() {
        let mut settings: AppSettings = serde_json::from_value(json!({ "libraryRoot": "D:/assets" })).unwrap();
        assert!(settings.plugin_settings.is_empty());
        settings.set_plugin_setting("com.example.a", "engine", json!("D:/engine.exe"));
        settings.set_plugin_setting("com.example.a", "size", json!("2048"));
        settings.set_plugin_setting("com.example.b", "engine", json!("  "));
        let restored: AppSettings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.plugin_settings["com.example.a"]["engine"], "D:/engine.exe");
        assert!(!restored.plugin_settings.contains_key("com.example.b"));
        settings.set_plugin_setting("com.example.a", "engine", json!(null));
        settings.set_plugin_setting("com.example.a", "size", json!(""));
        assert!(settings.plugin_settings.is_empty());
        assert!(!serde_json::to_string(&settings).unwrap().contains("pluginSettings"));
    }

    #[test]
    fn saved_exports_round_trip_and_old_settings_still_load() {
        let mut settings: AppSettings = serde_json::from_value(json!({ "libraryRoot": "D:/assets", "fbx2gltfPath": "D:/convert.exe" })).unwrap();
        assert!(settings.saved_exports.is_empty());
        settings.saved_exports.push(json!({ "id":"spy", "name":"Spygame", "kind":"model", "exporterId":"com.toybox.placer", "targetDir":"D:/game", "variantPolicy":"all", "config":{"format":"glb","subDir":"assets"} }));
        let restored: AppSettings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.saved_exports, settings.saved_exports);
        assert_eq!(restored.library_root, None, "the legacy root is read, never written");
        assert_eq!(restored.fbx2gltf_path.as_deref(), Some("D:/convert.exe"));
    }

    #[test]
    fn plugins_are_on_unless_turned_off_and_the_choice_survives_a_restart() {
        let mut settings: AppSettings = serde_json::from_value(json!({ "libraryRoot": "D:/assets" })).unwrap();
        assert!(settings.disabled_plugins.is_empty());
        settings.disabled_plugins = vec!["com.example.a".to_string(), "com.toybox.skybox-core".to_string()];
        let restored: AppSettings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.disabled_plugins, settings.disabled_plugins);
    }
}
