//! First-run seeding of bundled plugins. The app ships its built-in plugins
//! (e.g. the glTF importer) as bundle resources; on startup we copy each into the
//! user's `<app-data>/plugins/<id>` so the running app discovers it — exactly
//! where `link-plugins.mjs` puts dev plugins. A plugin already present is left
//! untouched, so a user's own install (or local edits) is never overwritten.

use std::path::Path;

use tauri::{AppHandle, Manager};

use crate::error::AppResult;
use crate::infra::appdata;

/// The bundled plugin ids to seed. Each must have a matching `bundle.resources`
/// entry (`plugins/<id>/manifest.json` + `plugins/<id>/dist`) in tauri.conf.json.
const BUNDLED_PLUGINS: &[&str] = &["com.toybox.gltf-importer"];

/// Copy any not-yet-installed bundled plugin into the app-data plugins dir.
/// Best-effort and idempotent: a present plugin is skipped, and a copy failure
/// for one plugin doesn't abort startup (logged via the returned error only when
/// the whole resource root is unreadable).
pub fn seed_bundled_plugins(app: &AppHandle) -> AppResult<()> {
    let Ok(resources) = app.path().resource_dir() else {
        return Ok(()); // no resource dir (unusual) — nothing to seed
    };
    let dest_root = appdata::plugins_dir(app)?;
    std::fs::create_dir_all(&dest_root)?;

    for id in BUNDLED_PLUGINS {
        // A failed copy for one plugin shouldn't break the app; drop it.
        let _ = seed_one(&resources.join("plugins").join(id), &dest_root.join(id));
    }
    Ok(())
}

/// Copy one bundled plugin `src` into `dst`, unless it isn't bundled here or is
/// already installed (then leave the existing one untouched). Returns whether a
/// copy happened.
fn seed_one(src: &Path, dst: &Path) -> std::io::Result<bool> {
    if !src.is_dir() || dst.exists() {
        return Ok(false);
    }
    copy_dir(src, dst)?;
    Ok(true)
}

/// Recursively copy `src` into `dst`.
fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_when_absent_and_never_overwrites() {
        let tmp = std::env::temp_dir().join("toybox_bundled_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let src = tmp.join("src/com.toybox.gltf-importer");
        std::fs::create_dir_all(src.join("dist/ui")).unwrap();
        std::fs::write(src.join("manifest.json"), "{}").unwrap();
        std::fs::write(src.join("dist/ui/Import.js"), "bundled").unwrap();

        let dst = tmp.join("appdata/com.toybox.gltf-importer");

        // First run: absent → seeded, files land.
        assert!(seed_one(&src, &dst).unwrap(), "first run seeds");
        assert_eq!(
            std::fs::read_to_string(dst.join("dist/ui/Import.js")).unwrap(),
            "bundled"
        );

        // Simulate the user editing their installed copy, then a second run:
        // present → skipped, the user's edit survives.
        std::fs::write(dst.join("dist/ui/Import.js"), "user-edit").unwrap();
        assert!(!seed_one(&src, &dst).unwrap(), "second run skips existing");
        assert_eq!(
            std::fs::read_to_string(dst.join("dist/ui/Import.js")).unwrap(),
            "user-edit",
            "never overwrites an installed plugin"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
