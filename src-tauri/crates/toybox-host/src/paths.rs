//! Where Toybox keeps its per-user data.

use std::path::{Path, PathBuf};

use toybox_core::error::{AppError, AppResult};

/// The two per-user folders: `app_data` (settings, plugins, per-library data) and
/// `cache` (downloaded files of remote libraries, plugin scratch folders), which
/// the system may clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPaths {
    pub app_data: PathBuf,
    pub cache: PathBuf,
}

impl HostPaths {
    pub fn new(app_data: impl Into<PathBuf>, cache: impl Into<PathBuf>) -> Self {
        HostPaths { app_data: app_data.into(), cache: cache.into() }
    }

    /// The folders Tauri's `app_data_dir()` / `app_cache_dir()` give the app with
    /// this identifier: `<data dir>/<id>` and `<cache dir>/<id>`, where the base
    /// folders are `%APPDATA%` and `%LOCALAPPDATA%` on Windows,
    /// `$XDG_DATA_HOME` (`~/.local/share`) and `~/.cache` on Linux and
    /// `~/Library/Application Support` and `~/Library/Caches` on macOS.
    pub fn for_app_id(identifier: &str) -> AppResult<Self> {
        let data = dirs::data_dir().ok_or_else(|| AppError::msg("could not find the user's data folder"))?;
        let cache = dirs::cache_dir().ok_or_else(|| AppError::msg("could not find the user's cache folder"))?;
        Ok(Self::under(&data, &cache, identifier))
    }

    fn under(data_base: &Path, cache_base: &Path, identifier: &str) -> Self {
        HostPaths { app_data: data_base.join(identifier), cache: cache_base.join(identifier) }
    }

    /// Everything under one folder (`app_data = dir`, `cache = dir/cache`), for
    /// tests and automation.
    pub fn for_home(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        HostPaths { cache: dir.join("cache"), app_data: dir }
    }

    /// `<app-data>/settings.json` (holds the library registry).
    pub fn settings_path(&self) -> PathBuf {
        toybox_core::infra::appdata::settings_path(&self.app_data)
    }

    /// `<app-data>/plugins`: one subfolder per installed plugin. The `plugin://`
    /// protocol and the loader's source reads are jailed to this root.
    pub fn plugins_dir(&self) -> PathBuf {
        self.app_data.join("plugins")
    }

    /// `<cache>/libraries`: downloaded files of remote libraries.
    pub fn remote_files_dir(&self) -> PathBuf {
        self.cache.join("libraries")
    }

    /// `<cache>/plugin-scratch`: parent of the plugins' private scratch folders.
    pub fn plugin_scratch_root(&self) -> PathBuf {
        self.cache.join("plugin-scratch")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_folders_are_the_base_folders_plus_the_identifier() {
        let paths = HostPaths::under(Path::new("/data"), Path::new("/cache"), "com.toybox.app");
        assert_eq!(paths.app_data, Path::new("/data").join("com.toybox.app"));
        assert_eq!(paths.cache, Path::new("/cache").join("com.toybox.app"));
        assert_eq!(paths.plugins_dir(), paths.app_data.join("plugins"));
        assert_eq!(paths.settings_path(), paths.app_data.join("settings.json"));
        assert_eq!(paths.remote_files_dir(), paths.cache.join("libraries"));
    }

    #[test]
    fn a_home_holds_everything() {
        let paths = HostPaths::for_home("/h");
        assert_eq!(paths.app_data, Path::new("/h"));
        assert_eq!(paths.cache, Path::new("/h").join("cache"));
    }

    #[test]
    fn the_real_folders_exist_for_the_current_user() {
        let paths = HostPaths::for_app_id("com.toybox.app").unwrap();
        assert!(paths.app_data.ends_with("com.toybox.app") && paths.cache.ends_with("com.toybox.app"));
    }
}
