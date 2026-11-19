//! Persisted application settings at `<app-data>/settings.json`. The schema and
//! the path-based read/write live in `toybox-core`; these methods resolve the
//! path from the host and add the small settings operations.

use toybox_core::error::AppResult;
pub use toybox_core::infra::settings::AppSettings;

use crate::host::Host;
use crate::plugins;

impl Host {
    /// Load settings, defaulting to empty whenever the file can't be read as our
    /// schema (see `toybox_core::infra::settings::load_from`). Never fails on a bad
    /// file: it must be safe to call at start-up.
    pub fn load_settings(&self) -> AppResult<AppSettings> {
        toybox_core::infra::settings::load_from(&self.paths.settings_path())
    }

    pub fn save_settings(&self, settings: &AppSettings) -> AppResult<()> {
        toybox_core::infra::settings::save_to(&self.paths.settings_path(), settings)
    }

    pub fn saved_exports(&self) -> AppResult<Vec<serde_json::Value>> {
        Ok(self.load_settings()?.saved_exports)
    }

    pub fn set_saved_exports(&self, exports: Vec<serde_json::Value>) -> AppResult<()> {
        let mut s = self.load_settings()?;
        s.saved_exports = exports;
        self.save_settings(&s)
    }

    /// Turn an installed plugin on or off. The plugin's files are untouched.
    /// Turning a plugin on also turns on what it depends on; turning one off is
    /// refused while a switched-on plugin needs it. Returns the sorted ids whose
    /// switch changed.
    pub fn set_plugin_enabled(&self, id: &str, enabled: bool) -> AppResult<Vec<String>> {
        let mut s = self.load_settings()?;
        let plan = plugins::plan_plugin_enabled(&self.list_plugins()?, &s.disabled_plugins, id, enabled)?;
        s.disabled_plugins = plan.disabled;
        self.save_settings(&s)?;
        Ok(plan.changed)
    }

    /// The configured FBX2glTF executable path, or `None` if unset.
    pub fn fbx2gltf_path(&self) -> AppResult<Option<String>> {
        Ok(self.load_settings()?.fbx2gltf_path)
    }

    /// Set (or, with `None`, clear) the FBX2glTF executable path. The path is
    /// trusted (a local single-user tool): this only persists, keeping other settings.
    pub fn set_fbx2gltf_path(&self, path: Option<String>) -> AppResult<()> {
        let mut s = self.load_settings()?;
        s.fbx2gltf_path = path.filter(|p| !p.trim().is_empty());
        self.save_settings(&s)
    }
}
