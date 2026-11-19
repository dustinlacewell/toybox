//! Install and update app-owned bundled plugins. Receipts detect local edits;
//! replaced legacy/bundled copies remain in timestamped app-data backups.

use std::path::Path;
use std::collections::BTreeMap;
use serde::{Serialize,Deserialize};

use toybox_core::error::AppResult;

use crate::host::Host;

/// The bundled plugin ids to seed. Each must have a matching `bundle.resources`
/// entry (`plugins/<id>/manifest.json` + `plugins/<id>/dist`) in tauri.conf.json.
const BUNDLED_PLUGINS: &[&str] = &[
    "com.toybox.model-core", "com.toybox.model-gltf", "com.toybox.animation-core",
    "com.toybox.material-core",
    "com.toybox.audio-core", "com.toybox.skybox-core",
    "com.toybox.rig-synty-polygon", "com.toybox.rig-synty-sidekick", "com.toybox.rig-synty-unreal", "com.toybox.rig-quaternius",
];

/// A bundled plugin that no longer ships: its old id and the bundled plugins
/// that took over its work (none when the work was dropped).
pub type Retirement = (&'static str, &'static [&'static str]);

/// Bundled plugins that no longer ship, each with its replacements. To retire
/// another plugin add one line; nothing else needs to change:
///
/// - an unmodified managed copy of the old plugin is moved to the backups folder
///   when the bundled plugins are seeded; a linked or edited copy stays on disk;
/// - the old plugin is never listed while every replacement is installed (with
///   no replacements it is never listed);
/// - when the user had the old plugin switched off, its replacements start
///   switched off too;
/// - library items that record the old id in `origin.plugin` follow the
///   replacements (the manifests of the replacements list it in `replaces`).
pub const RETIRED_PLUGINS: &[Retirement] = &[
    ("com.toybox.gltf-importer", &["com.toybox.model-core", "com.toybox.model-gltf", "com.toybox.animation-core"]),
    // Retired with no replacement: format plugins that are no longer bundled.
    ("com.toybox.model-fbx", &[]),
    ("com.toybox.model-obj", &[]),
    // Renamed to the `<type>-core` pattern (the new ids start at a new major version).
    ("com.toybox.material-importer", &["com.toybox.material-core"]),
    ("com.toybox.audio-importer", &["com.toybox.audio-core"]),
    ("com.toybox.skybox", &["com.toybox.skybox-core"]),
    // Split: one plugin per rig.
    ("com.toybox.humanoid-rigs", &["com.toybox.rig-synty-polygon", "com.toybox.rig-synty-sidekick", "com.toybox.rig-synty-unreal", "com.toybox.rig-quaternius"]),
];

/// The retired ids that plugin `id` replaces, sorted.
pub fn replaces(retired: &[Retirement], id: &str) -> Vec<String> {
    let mut old: Vec<String> = retired.iter().filter(|(_, replacements)| replacements.contains(&id)).map(|(old, _)| old.to_string()).collect();
    old.sort();
    old
}

/// Whether a retired plugin is out of the way: each of its replacements is
/// installed, which holds trivially when it has none.
pub fn is_superseded(replacements: &[&str], installed: impl Fn(&str) -> bool) -> bool {
    replacements.iter().all(|id| installed(id))
}

impl Host {
    /// Copy any not-yet-installed bundled plugin from `<resources>/plugins/<id>` into
    /// the plugins folder. Best-effort and idempotent: a present plugin is skipped,
    /// and a copy failure for one plugin doesn't abort startup.
    pub fn seed_bundled_plugins(&self, resources: &Path) -> AppResult<()> {
        let dest_root = self.paths.plugins_dir();
        std::fs::create_dir_all(&dest_root)?;

        for id in BUNDLED_PLUGINS {
            // A failed copy for one plugin shouldn't break the app; drop it.
            let _ = seed_one(&resources.join("plugins").join(id), &dest_root.join(id));
        }
        for (old, replacements) in RETIRED_PLUGINS {
            let _ = retire_one(&dest_root, old);
            // The user's switch for the old plugin carries over to its replacements.
            let present: Vec<&str> = replacements.iter().copied().filter(|id| dest_root.join(id).is_dir()).collect();
            if !present.is_empty() {
                if let Ok(mut settings) = self.load_settings() {
                    // `|` not `||`: both carry-overs run.
                    if carry_over_disabled(&mut settings.disabled_plugins, old, &present) | carry_over_plugin_settings(&mut settings.plugin_settings, old, &present) {
                        let _ = self.save_settings(&settings);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Hand the stored preferences of `old` (`plugin_settings` is keyed by plugin id)
/// to each of its `replacements`, then drop the old entry; true when it changed.
/// A value the replacement already has stays: what the user set under the new id
/// is newer. Settings a replacement does not declare are ignored when read.
fn carry_over_plugin_settings(stored: &mut BTreeMap<String, serde_json::Map<String, serde_json::Value>>, old: &str, replacements: &[&str]) -> bool {
    let Some(values) = stored.remove(old) else { return false };
    for new in replacements {
        let target = stored.entry(new.to_string()).or_default();
        for (key, value) in &values {
            target.entry(key.clone()).or_insert_with(|| value.clone());
        }
        if target.is_empty() { stored.remove(*new); }
    }
    true
}

/// Replace `old` by its `replacements` in the switched-off ids; true when it changed.
fn carry_over_disabled(disabled: &mut Vec<String>, old: &str, replacements: &[&str]) -> bool {
    if !disabled.iter().any(|id| id == old) {
        return false;
    }
    disabled.retain(|id| id != old);
    for new in replacements {
        if !disabled.iter().any(|id| id == new) {
            disabled.push(new.to_string());
        }
    }
    disabled.sort();
    true
}

/// Move a retired plugin's folder to the backups folder when it is an unmodified
/// managed install. A linked or locally modified copy stays where it is.
fn retire_one(plugins_dir: &Path, old: &str) -> std::io::Result<bool> {
    let dst = plugins_dir.join(old);
    if !dst.exists() || linked(&dst)? || !dst.join(RECEIPT).is_file() {return Ok(false);}
    let receipt:Receipt=serde_json::from_slice(&std::fs::read(dst.join(RECEIPT))?)?;
    if receipt.version!=1 || receipt.files!=inventory(&dst)? {return Ok(false);}
    let appdata=plugins_dir.parent().ok_or_else(||std::io::Error::other("Invalid plugin destination"))?;
    let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(std::io::Error::other)?.as_nanos();
    let backup=appdata.join("plugin-backups").join(format!("{old}-{stamp}"));
    std::fs::create_dir_all(backup.parent().unwrap())?;
    std::fs::rename(&dst,&backup)?;
    Ok(true)
}

const RECEIPT:&str=".toybox-bundle.json";
#[derive(Serialize,Deserialize)]
struct Receipt {version:u32,files:BTreeMap<String,u64>}

pub(crate) fn linked(path:&Path)->std::io::Result<bool> {
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
                files.insert(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\',"/"),toybox_core::domain::rig::hash(&std::fs::read(path)?));
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
pub(crate) fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
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
    fn retired_plugins_are_replaced_by_bundled_ones() {
        let mut olds: Vec<&str> = RETIRED_PLUGINS.iter().map(|(old, _)| *old).collect();
        olds.sort();
        olds.dedup();
        assert_eq!(olds.len(), RETIRED_PLUGINS.len(), "a plugin is retired once");
        for (old, replacements) in RETIRED_PLUGINS {
            assert!(!BUNDLED_PLUGINS.contains(old), "{old} is retired but still bundled");
            for new in *replacements {
                assert!(BUNDLED_PLUGINS.contains(new), "{new} replaces {old} but is not bundled");
            }
        }
    }

    #[test]
    fn the_model_split_is_in_the_retirement_table() {
        let all = ["com.toybox.model-core", "com.toybox.model-gltf", "com.toybox.animation-core"];
        let table: std::collections::HashMap<_, _> = RETIRED_PLUGINS.iter().copied().collect();
        assert_eq!(table["com.toybox.gltf-importer"], all.as_slice());
        // FBX and OBJ import left with no successor.
        assert!(table["com.toybox.model-fbx"].is_empty());
        assert!(table["com.toybox.model-obj"].is_empty());
        for id in all {
            assert!(BUNDLED_PLUGINS.contains(&id));
            assert_eq!(replaces(RETIRED_PLUGINS, id), vec!["com.toybox.gltf-importer"]);
        }
    }

    #[test]
    fn the_renames_and_the_rig_split_are_in_the_retirement_table() {
        let table: std::collections::HashMap<_, _> = RETIRED_PLUGINS.iter().copied().collect();
        assert_eq!(table["com.toybox.material-importer"], ["com.toybox.material-core"].as_slice());
        assert_eq!(table["com.toybox.audio-importer"], ["com.toybox.audio-core"].as_slice());
        assert_eq!(table["com.toybox.skybox"], ["com.toybox.skybox-core"].as_slice());
        let rigs = ["com.toybox.rig-synty-polygon", "com.toybox.rig-synty-sidekick", "com.toybox.rig-synty-unreal", "com.toybox.rig-quaternius"];
        assert_eq!(table["com.toybox.humanoid-rigs"], rigs.as_slice());
        for id in rigs {
            assert_eq!(replaces(RETIRED_PLUGINS, id), vec!["com.toybox.humanoid-rigs"]);
        }
        assert_eq!(replaces(RETIRED_PLUGINS, "com.toybox.skybox-core"), vec!["com.toybox.skybox"]);
    }

    #[test]
    fn every_bundled_manifest_matches_its_folder_and_finds_its_dependencies() {
        let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let manifests: std::collections::HashMap<&str, serde_json::Value> = BUNDLED_PLUGINS.iter()
            .map(|id| (*id, serde_json::from_slice(&std::fs::read(plugins.join(id).join("manifest.json")).unwrap_or_else(|_| panic!("{id} has no manifest"))).unwrap()))
            .collect();
        for (id, manifest) in &manifests {
            assert_eq!(manifest["id"], *id, "the manifest id is the folder name");
            let version = semver::Version::parse(manifest["version"].as_str().unwrap()).unwrap();
            for (needed, range) in manifest["dependencies"].as_object().into_iter().flatten() {
                let dependency = manifests.get(needed.as_str()).unwrap_or_else(|| panic!("{id} needs {needed}, which is not bundled"));
                let installed = semver::Version::parse(dependency["version"].as_str().unwrap()).unwrap();
                assert!(semver::VersionReq::parse(range.as_str().unwrap()).unwrap().matches(&installed), "{id} {version} needs {needed} {range}, bundled {installed}");
            }
        }
    }

    #[test]
    fn the_settings_of_a_retired_plugin_carry_over() {
        let values = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), serde_json::json!(v))).collect::<serde_json::Map<_, _>>();
        let mut stored = BTreeMap::new();
        stored.insert("old".to_string(), values(&[("engine", "D:/old.exe"), ("size", "12")]));
        stored.insert("other".to_string(), values(&[("a", "b")]));
        assert!(carry_over_plugin_settings(&mut stored, "old", &["new"]));
        assert!(!stored.contains_key("old"));
        assert_eq!(stored["new"], values(&[("engine", "D:/old.exe"), ("size", "12")]));
        assert_eq!(stored["other"], values(&[("a", "b")]));
        // Done once: the old id is gone, so nothing changes on the next start.
        assert!(!carry_over_plugin_settings(&mut stored, "old", &["new"]));
        // Values already stored under the new id win; the rest come across.
        stored.insert("older".to_string(), values(&[("engine", "D:/older.exe"), ("renderer", "CPU")]));
        assert!(carry_over_plugin_settings(&mut stored, "older", &["new"]));
        assert_eq!(stored["new"], values(&[("engine", "D:/old.exe"), ("size", "12"), ("renderer", "CPU")]));
        // A split hands the settings to every replacement.
        stored.insert("split".to_string(), values(&[("k", "v")]));
        assert!(carry_over_plugin_settings(&mut stored, "split", &["a", "b"]));
        assert_eq!(stored["a"], values(&[("k", "v")]));
        assert_eq!(stored["b"], values(&[("k", "v")]));
        // An old plugin with nothing stored leaves no empty entry behind.
        stored.insert("blank".to_string(), serde_json::Map::new());
        assert!(carry_over_plugin_settings(&mut stored, "blank", &["c"]));
        assert!(!stored.contains_key("blank") && !stored.contains_key("c"));
    }

    #[test]
    fn the_switch_of_the_retired_rig_plugin_turns_all_four_rigs_off() {
        let rigs = RETIRED_PLUGINS.iter().find(|(old, _)| *old == "com.toybox.humanoid-rigs").unwrap().1;
        let mut disabled = vec!["com.toybox.humanoid-rigs".to_string()];
        assert!(carry_over_disabled(&mut disabled, "com.toybox.humanoid-rigs", rigs));
        assert_eq!(disabled.len(), 4);
        assert!(rigs.iter().all(|id| disabled.iter().any(|d| d == id)));
    }

    #[test]
    fn a_retired_plugin_is_superseded_only_when_every_replacement_is_installed() {
        let installed = |ids: &'static [&'static str]| move |id: &str| ids.contains(&id);
        assert!(is_superseded(&["a", "b"], installed(&["a", "b", "c"])));
        assert!(!is_superseded(&["a", "b"], installed(&["a"])));
        assert!(is_superseded(&[], installed(&[])), "nothing replaces it, nothing keeps it");
        let table: &[Retirement] = &[("old", &["a", "b"]), ("gone", &[]), ("older", &["b"])];
        assert_eq!(replaces(table, "b"), vec!["old", "older"]);
        assert_eq!(replaces(table, "a"), vec!["old"]);
    }

    #[test]
    fn retirement_moves_only_unmodified_managed_installs() {
        let tmp=std::env::temp_dir().join(format!("toybox-bundle-retire-{}",std::process::id()));
        let _=std::fs::remove_dir_all(&tmp);
        let plugins=tmp.join("app/plugins");
        let src=tmp.join("resources/old.plugin");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("manifest.json"),br#"{"id":"old.plugin"}"#).unwrap();
        assert!(!retire_one(&plugins,"old.plugin").unwrap(),"nothing installed");
        // Managed copy: seeded with a receipt, so it is moved to backups.
        let dst=plugins.join("old.plugin");
        assert!(seed_one(&src,&dst).unwrap());
        assert!(retire_one(&plugins,"old.plugin").unwrap());
        assert!(!dst.exists());
        assert_eq!(std::fs::read_dir(tmp.join("app/plugin-backups")).unwrap().count(),1);
        // Modified copy: stays.
        assert!(seed_one(&src,&dst).unwrap());
        std::fs::write(dst.join("manifest.json"),br#"{"id":"old.plugin","edited":true}"#).unwrap();
        assert!(!retire_one(&plugins,"old.plugin").unwrap());
        assert!(dst.join("manifest.json").is_file());
        // A copy without a receipt is not a managed install.
        std::fs::remove_file(dst.join(RECEIPT)).unwrap();
        assert!(!retire_one(&plugins,"old.plugin").unwrap());
        assert!(dst.is_dir());
        std::fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn the_switch_of_a_retired_plugin_carries_over() {
        let mut disabled=vec!["a".to_string(),"old".to_string()];
        assert!(carry_over_disabled(&mut disabled,"old",&["new"]));
        assert_eq!(disabled,vec!["a","new"]);
        assert!(!carry_over_disabled(&mut disabled,"old",&["new"]));
        let mut both=vec!["new".to_string(),"old".to_string()];
        assert!(carry_over_disabled(&mut both,"old",&["new"]));
        assert_eq!(both,vec!["new"]);
    }

    #[test]
    fn the_switch_of_a_retired_plugin_carries_over_to_every_replacement() {
        let mut disabled=vec!["other".to_string(),"com.toybox.gltf-importer".to_string()];
        let replacements=RETIRED_PLUGINS.iter().find(|(old,_)|*old=="com.toybox.gltf-importer").unwrap().1;
        assert!(carry_over_disabled(&mut disabled,"com.toybox.gltf-importer",replacements));
        assert_eq!(disabled,vec!["com.toybox.animation-core","com.toybox.model-core","com.toybox.model-gltf","other"]);
        // On the next start the old id is gone, so the user's later choices stand.
        assert!(!carry_over_disabled(&mut disabled,"com.toybox.gltf-importer",replacements));
        // A plugin that was on stays on.
        let mut on=vec!["other".to_string()];
        assert!(!carry_over_disabled(&mut on,"com.toybox.gltf-importer",replacements));
        assert_eq!(on,vec!["other"]);
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
