//! Installing and removing plugins: a plugin is a folder under `<app-data>/plugins/<id>`.

use std::path::{Path, PathBuf};

use toybox_core::error::{AppError, AppResult};

use crate::bundled::{copy_dir, linked};
use crate::host::Host;
use crate::plugin_services::valid_plugin_id;

/// What an install did.
#[derive(Debug, Clone)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub linked: bool,
}

/// Link `link` to the folder `target`: a symbolic link, or a junction on Windows (which needs no privilege).
fn link_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return Ok(());
        }
        use std::os::windows::process::CommandExt;
        let status = std::process::Command::new("cmd")
            .arg("/C").arg("mklink").arg("/J").arg(link).arg(target)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .status()?;
        if status.success() { Ok(()) } else { Err(std::io::Error::other("could not create a junction")) }
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link)
    }
}

impl Host {
    /// Install the plugin folder `source` (which must hold a `manifest.json` with an id,
    /// a name and a version) as `<plugins>/<id>`: a copy, or with `link` a link to the
    /// folder (for development). Refuses an id that is already installed.
    pub fn install_plugin(&self, source: &Path, link: bool) -> AppResult<Installed> {
        let manifest_path = source.join("manifest.json");
        let text = std::fs::read_to_string(&manifest_path).map_err(|_| AppError::msg(format!("{} has no manifest.json", source.display())))?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(|error| AppError::msg(format!("manifest.json is not valid JSON: {error}")))?;
        let field = |name: &str| doc.get(name).and_then(|value| value.as_str()).map(str::to_string).filter(|text| !text.trim().is_empty());
        let id = field("id").ok_or_else(|| AppError::msg("the manifest has no id"))?;
        let name = field("name").ok_or_else(|| AppError::msg("the manifest has no name"))?;
        let version = field("version").ok_or_else(|| AppError::msg("the manifest has no version"))?;
        if !valid_plugin_id(&id) {
            return Err(AppError::msg(format!("'{id}' is not a valid plugin id (letters, digits, dots, dashes and underscores)")));
        }
        let dest = self.paths.plugins_dir().join(&id);
        if dest.exists() || std::fs::symlink_metadata(&dest).is_ok() {
            return Err(AppError::msg(format!("plugin '{id}' is already installed (remove it first)")));
        }
        std::fs::create_dir_all(self.paths.plugins_dir())?;
        if link {
            let target = std::path::absolute(source)?;
            link_dir(&target, &dest).map_err(|error| AppError::msg(format!("could not link {}: {error}", dest.display())))?;
        } else {
            let stage = self.paths.app_data.join("plugin-staging").join(format!("{id}-install"));
            let _ = std::fs::remove_dir_all(&stage);
            copy_dir(source, &stage)?;
            if let Err(error) = std::fs::rename(&stage, &dest) {
                let _ = std::fs::remove_dir_all(&stage);
                return Err(error.into());
            }
        }
        Ok(Installed { id, name, version, path: dest, linked: link })
    }

    /// Remove an installed plugin: its folder, or only the link of a linked plugin (the
    /// folder it points at is left alone). Refused while a switched-on plugin needs it.
    /// Its stored settings and its switch are forgotten.
    pub fn remove_plugin(&self, id: &str) -> AppResult<()> {
        if !valid_plugin_id(id) {
            return Err(AppError::msg(format!("'{id}' is not a plugin id")));
        }
        let dir = self.paths.plugins_dir().join(id);
        if std::fs::symlink_metadata(&dir).is_err() {
            return Err(AppError::msg(format!("plugin '{id}' is not installed")));
        }
        let plugins = self.list_plugins()?;
        let needing: Vec<&str> = plugins.iter().filter(|plugin| plugin.enabled && plugin.id != id && plugin.dependencies.contains_key(id)).map(|plugin| plugin.name.as_str()).collect();
        if !needing.is_empty() {
            return Err(AppError::msg(format!("{id} is needed by {}. Turn them off or remove them first.", needing.join(", "))));
        }
        if linked(&dir)? {
            // Junctions and directory links go with remove_dir; file links with remove_file.
            std::fs::remove_dir(&dir).or_else(|_| std::fs::remove_file(&dir))?;
        } else {
            std::fs::remove_dir_all(&dir)?;
        }
        let mut settings = self.load_settings()?;
        let before = (settings.disabled_plugins.len(), settings.plugin_settings.len());
        settings.disabled_plugins.retain(|other| other != id);
        settings.plugin_settings.remove(id);
        if before != (settings.disabled_plugins.len(), settings.plugin_settings.len()) {
            self.save_settings(&settings)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::HostPaths;
    use std::sync::Arc;
    use toybox_remote::secrets::MemoryStore;

    fn world(tag: &str) -> (Host, PathBuf) {
        let base = std::env::temp_dir().join(format!("toybox-plugin-admin-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let host = Host::open_lazy(HostPaths::for_home(base.join("home")), Arc::new(MemoryStore::new()), None).unwrap();
        (host, base)
    }

    fn plugin(base: &Path, id: &str, extra: serde_json::Value) -> PathBuf {
        let dir = base.join(format!("src-{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut manifest = serde_json::json!({ "id": id, "name": id, "version": "1.0.0" });
        for (key, value) in extra.as_object().into_iter().flatten() {
            manifest[key] = value.clone();
        }
        std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
        std::fs::write(dir.join("index.js"), "export default {}").unwrap();
        dir
    }

    #[test]
    fn a_plugin_is_copied_in_listed_and_removed_with_its_settings() {
        let (host, base) = world("copy");
        let source = plugin(&base, "test.one", serde_json::json!({}));
        let installed = host.install_plugin(&source, false).unwrap();
        assert_eq!((installed.id.as_str(), installed.linked), ("test.one", false));
        assert!(host.paths().plugins_dir().join("test.one").join("index.js").is_file());
        assert_eq!(host.list_plugins().unwrap().len(), 1);
        assert!(host.install_plugin(&source, false).unwrap_err().to_string().contains("already installed"));
        host.set_plugin_enabled("test.one", false).unwrap();
        host.remove_plugin("test.one").unwrap();
        assert!(host.list_plugins().unwrap().is_empty());
        assert!(host.load_settings().unwrap().disabled_plugins.is_empty(), "its switch is forgotten");
        assert!(host.remove_plugin("test.one").is_err());
        assert!(host.remove_plugin("../escape").is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_folder_without_a_good_manifest_is_refused() {
        let (host, base) = world("bad");
        let empty = base.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(host.install_plugin(&empty, false).is_err());
        let bad = base.join("bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("manifest.json"), r#"{"id":"../x","name":"x","version":"1"}"#).unwrap();
        assert!(host.install_plugin(&bad, false).unwrap_err().to_string().contains("not a valid plugin id"));
        std::fs::write(bad.join("manifest.json"), r#"{"id":"x"}"#).unwrap();
        assert!(host.install_plugin(&bad, false).is_err());
        assert!(!host.paths().plugins_dir().exists() || std::fs::read_dir(host.paths().plugins_dir()).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_plugin_another_one_needs_cannot_be_removed() {
        let (host, base) = world("needs");
        host.install_plugin(&plugin(&base, "test.core", serde_json::json!({})), false).unwrap();
        host.install_plugin(&plugin(&base, "test.addon", serde_json::json!({ "dependencies": { "test.core": "*" } })), false).unwrap();
        let error = host.remove_plugin("test.core").unwrap_err().to_string();
        assert!(error.contains("is needed by"), "{error}");
        host.remove_plugin("test.addon").unwrap();
        host.remove_plugin("test.core").unwrap();
        let _ = std::fs::remove_dir_all(base);
    }
}
