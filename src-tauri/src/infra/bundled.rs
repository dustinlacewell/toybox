//! Install and update app-owned bundled plugins. Receipts detect local edits;
//! replaced legacy/bundled copies remain in timestamped app-data backups.

use std::path::Path;
use std::collections::BTreeMap;
use serde::{Serialize,Deserialize};

use tauri::{AppHandle, Manager};

use crate::error::AppResult;
use crate::infra::appdata;

/// The bundled plugin ids to seed. Each must have a matching `bundle.resources`
/// entry (`plugins/<id>/manifest.json` + `plugins/<id>/dist`) in tauri.conf.json.
const BUNDLED_PLUGINS: &[&str] = &["com.toybox.gltf-importer", "com.toybox.material-importer", "com.toybox.humanoid-rigs", "com.toybox.audio-importer"];

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

const RECEIPT:&str=".toybox-bundle.json";
#[derive(Serialize,Deserialize)]
struct Receipt {version:u32,files:BTreeMap<String,u64>}

fn linked(path:&Path)->std::io::Result<bool> {
    let metadata=std::fs::symlink_metadata(path)?;
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        return Ok(metadata.file_attributes() & 0x400 != 0);
    }
    #[cfg(not(windows))] {Ok(metadata.file_type().is_symlink())}
}
fn inventory(root:&Path)->std::io::Result<BTreeMap<String,u64>> {
    fn walk(root:&Path,path:&Path,files:&mut BTreeMap<String,u64>)->std::io::Result<()> {
        for entry in std::fs::read_dir(path)? {
            let path=entry?.path();
            if linked(&path)? {return Err(std::io::Error::other("Linked plugin files are user-managed"));}
            if path.is_dir(){walk(root,&path,files)?;}
            else if path.file_name()!=Some(std::ffi::OsStr::new(RECEIPT)) {
                files.insert(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\',"/"),crate::domain::rig::hash(&std::fs::read(path)?));
            }
        }
        Ok(())
    }
    let mut files=BTreeMap::new();walk(root,root,&mut files)?;Ok(files)
}

/// Linked development plugins and modified managed installs stay untouched.
/// Legacy copies with the same bundled ID are backed up before being upgraded.
fn seed_one(src: &Path, dst: &Path) -> std::io::Result<bool> {
    if !src.is_dir() {return Ok(false);}
    if dst.exists() && linked(dst)? {return Ok(false);}
    let incoming=inventory(src)?;
    if dst.exists() {
        let current=inventory(dst)?;
        let bundled:serde_json::Value=serde_json::from_slice(&std::fs::read(src.join("manifest.json"))?)?;
        let installed:serde_json::Value=serde_json::from_slice(&std::fs::read(dst.join("manifest.json"))?)?;
        if let (Some(incoming),Some(existing))=(bundled["version"].as_str(),installed["version"].as_str()) {
            match (semver::Version::parse(incoming),semver::Version::parse(existing)) {
                (Ok(incoming),Ok(existing)) if existing<=incoming=>{},
                _=>return Ok(false),
            }
        }
        if dst.join(RECEIPT).is_file() {
            let receipt:Receipt=serde_json::from_slice(&std::fs::read(dst.join(RECEIPT))?)?;
            if receipt.version!=1 || receipt.files!=current {return Ok(false);}
        } else {
            if !bundled["id"].is_string() || bundled["id"]!=installed["id"] {return Ok(false);}
        }
        if current==incoming {
            std::fs::write(dst.join(RECEIPT),serde_json::to_vec(&Receipt{version:1,files:incoming})?)?;
            return Ok(false);
        }
    }
    let appdata=dst.parent().and_then(Path::parent).ok_or_else(||std::io::Error::other("Invalid plugin destination"))?;
    let name=dst.file_name().ok_or_else(||std::io::Error::other("Invalid plugin name"))?.to_string_lossy();
    let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(std::io::Error::other)?.as_nanos();
    let stage=appdata.join("plugin-staging").join(format!("{name}-{stamp}"));
    copy_dir(src,&stage)?;
    std::fs::write(stage.join(RECEIPT),serde_json::to_vec(&Receipt{version:1,files:incoming})?)?;
    let backup=appdata.join("plugin-backups").join(format!("{name}-{stamp}"));
    std::fs::create_dir_all(dst.parent().unwrap())?;
    if dst.exists(){std::fs::create_dir_all(backup.parent().unwrap())?;std::fs::rename(dst,&backup)?;}
    if let Err(error)=std::fs::rename(&stage,dst) {
        if backup.exists(){std::fs::rename(&backup,dst)?;}
        return Err(error);
    }
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
    fn bundled_ids_are_unique() {
        let mut ids: Vec<&str> = BUNDLED_PLUGINS.to_vec();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), BUNDLED_PLUGINS.len());
    }

    #[test]
    fn migration_upgrades_legacy_and_managed_bundles_with_backups() {
        let tmp=std::env::temp_dir().join(format!("toybox-bundle-upgrade-{}",std::process::id()));
        let src=tmp.join("resources/example");let dst=tmp.join("app/plugins/example");
        for folder in [&src,&dst] {std::fs::create_dir_all(folder).unwrap();std::fs::write(folder.join("manifest.json"),br#"{"id":"example"}"#).unwrap();}
        std::fs::write(src.join("index.js"),"new").unwrap();std::fs::write(dst.join("index.js"),"legacy").unwrap();
        assert!(seed_one(&src,&dst).unwrap());assert_eq!(std::fs::read_to_string(dst.join("index.js")).unwrap(),"new");
        assert!(dst.join(RECEIPT).exists());assert_eq!(std::fs::read_dir(tmp.join("app/plugin-backups")).unwrap().count(),1);
        std::fs::write(src.join("index.js"),"next release").unwrap();assert!(seed_one(&src,&dst).unwrap());
        assert_eq!(std::fs::read_to_string(dst.join("index.js")).unwrap(),"next release");
        assert_eq!(std::fs::read_dir(tmp.join("app/plugin-backups")).unwrap().count(),2);
        std::fs::remove_dir_all(tmp).unwrap();
    }

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
