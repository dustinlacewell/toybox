//! Delete library registrations and exclusively owned files.
//!
//! The cost depends on the items deleted: their rows are removed in one
//! transaction (`Store::apply`), the files only they use are found through the
//! file-reference table, and the safety copy holds just their entries.
use crate::{
    error::{AppError, AppResult},
    infra::{fsio, store::{self, Delta, Store}},
};
use std::path::Path;
#[cfg(test)]
use serde_json::Value;

/// Delete the catalog assets `ids` (top-level items) from the library at `root`,
/// with the files and thumbnails only they own. Returns the ids that left the
/// catalog: the items and their variants.
///
/// All or nothing: an id that is not in the catalog, or a registration that
/// changed meanwhile, refuses the whole request. Files are set aside before the
/// rows change and restored if the change fails.
pub fn delete_from_library(root: &Path, cache: &Path, ids: &[String]) -> AppResult<Vec<String>> {
    let mut trace = crate::infra::trace::Trace::new("delete");
    let store = Store::local(root, cache);
    if ids.is_empty() {
        return Err(AppError::msg("Select items to delete"));
    }
    if !store.catalog_exists() {
        return Err(AppError::msg("The library has no catalog yet. Reload before deleting items."));
    }
    let revision = store::manifest_revision(root)?;
    // Every id must be a catalog asset, and a manifest item.
    let assets = store.load_top_assets(ids)?;
    for id in ids {
        if !assets.iter().any(|asset| asset.id == *id) {
            return Err(AppError::msg(format!("Selected item is no longer in the library: {id}")));
        }
    }
    let entries = store.load_entries(ids)?;
    if ids.iter().any(|id| !entries.iter().any(|entry| entry["id"].as_str() == Some(id))) {
        return Err(AppError::msg("Library registrations changed. Rescan before deleting these items."));
    }
    let mut removed: Vec<String> = Vec::new();
    for asset in &assets {
        removed.push(asset.id.clone());
        removed.extend(asset.variants.iter().map(|variant| variant.id.clone()));
    }
    trace.phase("check ids");
    let mut delete_files = super::delete_files::exclusive_files_in(root, &store, ids)?;
    trace.phase("exclusive files");
    delete_files.extend(super::delete_files::exclusive_thumbnails_in(cache, &store, ids)?);
    trace.phase("exclusive thumbnails");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let config_dir = root.join("_library_config");
    let result = (|| {
        // Safety copies, as JSON documents: the deleted entries, and the deleted catalog assets.
        fsio::write_bytes(
            &config_dir.join(format!("catalog-before-delete-{stamp}.json")),
            &serde_json::to_vec_pretty(&serde_json::json!({ "assets": entries }))?,
        )?;
        let rigs = store.rigs_for(&assets)?;
        fsio::write_bytes(
            &cache.with_file_name(format!("catalog-before-delete-{stamp}.json")),
            &serde_json::to_vec_pretty(&serde_json::json!({ "schemaVersion": crate::config::SCHEMA_VERSION, "libraryRoot": root.to_string_lossy(), "rigs": rigs, "assets": assets }))?,
        )?;
        // Files are staged (set aside, restored if this fails) before the rows change,
        // and the rows change in one transaction that is refused if the manifest moved on.
        trace.phase("safety copies");
        let staged = super::delete_files::StagedFiles::stage(delete_files, stamp)?;
        trace.phase("stage files");
        let applied = store
            .apply(&Delta { remove_items: ids.to_vec(), clear_previews: removed.clone(), expect_manifest_revision: Some(revision), ..Default::default() })
            .map_err(|error| {
                if error.to_string().contains("changed while it was being edited") {
                    AppError::msg("Library changed while deleting. Retry after other library operations finish.")
                } else {
                    error
                }
            })?;
        trace.phase("rows");
        staged.erase()?;
        trace.phase("erase files");
        Ok(applied.removed_ids)
    })();
    if result.is_ok() {
        // Keep only the newest few of the safety copies (one beside the manifest, one beside the cache).
        crate::infra::retention::prune_delete_catalogs(&config_dir);
        if let Some(folder) = cache.parent() {
            crate::infra::retention::prune_delete_catalogs(folder);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn deletion_survives_rescan_erases_owned_files_and_clears_preview_references() {
        let folder = std::env::temp_dir().join(format!("toybox-delete-{}", std::process::id()));
        let root = folder.join("library-root");
        std::fs::create_dir_all(root.join("_library_config")).unwrap();
        let entry = |id: &str| json!({"id":id,"name":id,"kind":"material","pack":"p","category":"stone","maps":[{"role":"albedo","file":"shared.png"}]});
        let mut selected = entry("one");
        selected["variants"] = json!([entry("one-small")]);
        let manifest =
            json!({"schemaVersion":1,"custom":"preserved","assets":[selected,entry("two")]});
        store::write_manifest(&root, &manifest).unwrap();
        for id in ["one", "two", "one-small"] {
            fsio::write_bytes(
                &root.join(format!("library/p/materials/{id}/shared.png")),
                b"source",
            )
            .unwrap();
        }
        let mut catalog = super::super::catalog_build::build_catalog(
            root.to_str().unwrap(),
            crate::domain::seed::parse_seed_entries(&manifest).unwrap(),
        )
        .unwrap();
        catalog.assets[1].user.favorite = true;
        catalog.assets[1].user.preview_asset_id = Some("one-small".into());
        let cache = folder.join("catalog.json");
        for id in ["one", "one-small", "two"] {
            fsio::write_bytes(
                &folder.join(format!(
                    "thumbs/{}.png",
                    crate::infra::appdata::sanitize_id(id)
                )),
                b"cached preview",
            )
            .unwrap();
        }
        let store = Store::local(&root, &cache);
        store.save_catalog(&catalog).unwrap();
        let saved = serde_json::to_value(store.load_catalog().unwrap().unwrap()).unwrap();
        assert!(delete_from_library(&root, &cache, &["absent".into()]).is_err());
        assert_eq!(serde_json::to_value(store.load_catalog().unwrap().unwrap()).unwrap(), saved);
        delete_from_library(&root, &cache, &["one".into()]).unwrap();
        let result = store.load_catalog().unwrap().unwrap();
        assert_eq!(result.assets.len(), 1);
        assert_eq!(result.assets[0].id, "two");
        assert!(result.assets[0].user.favorite);
        assert!(result.assets[0].user.preview_asset_id.is_none());
        let updated: Value = store::read_manifest(&root).unwrap();
        assert_eq!(updated["custom"], "preserved");
        let fresh = super::super::catalog_build::build_catalog(
            root.to_str().unwrap(),
            crate::domain::seed::parse_seed_entries(&updated).unwrap(),
        )
        .unwrap();
        assert_eq!(fresh.assets.len(), 1);
        assert!(!root.join("library/p/materials/one/shared.png").exists());
        assert!(!root
            .join("library/p/materials/one-small/shared.png")
            .exists());
        assert!(root.join("library/p/materials/two/shared.png").exists());
        assert!(!folder.join("thumbs/one.png").exists());
        assert!(!folder.join("thumbs/one_small.png").exists());
        assert!(folder.join("thumbs/two.png").exists());
        assert!(std::fs::read_dir(&folder).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("catalog-before-delete-")));
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // The personal database cannot be written (another process holds it): the
            // delete is refused before anything is changed.
            let before_manifest = store::read_manifest(&root).unwrap();
            let before_cache = serde_json::to_value(store.load_catalog().unwrap().unwrap()).unwrap();
            let locked = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(store.user_path())
                .unwrap();
            assert!(delete_from_library(&root, &cache, &["two".into()]).is_err());
            assert_eq!(store::read_manifest(&root).unwrap(), before_manifest);
            assert_eq!(serde_json::to_value(store.load_catalog().unwrap().unwrap()).unwrap(), before_cache);
            assert_eq!(
                fsio::read_bytes(&root.join("library/p/materials/two/shared.png")).unwrap(),
                b"source"
            );
            assert!(folder.join("thumbs/two.png").exists());
            drop(locked);
        }
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn deleting_the_last_file_of_a_set_removes_its_folder_but_never_a_busy_or_shared_one() {
        let folder = std::env::temp_dir().join(format!("toybox-delete-sets-{}", std::process::id()));
        let root = folder.join("library-root");
        std::fs::create_dir_all(root.join("_library_config")).unwrap();
        let key = |c: char| c.to_string().repeat(64);
        let entry = |id: &str, set: &str| json!({"id":id,"name":id,"kind":"material","pack":"p","category":"stone","storagePath":format!("library/materials/{set}"),"maps":[{"role":"albedo","file":"a.png"}]});
        let manifest = json!({"schemaVersion":1,"storageVersion":1,"assets":[entry("one", &key('1')), entry("two", &key('2')), entry("three", &key('3'))]});
        store::write_manifest(&root, &manifest).unwrap();
        for set in ['1', '2', '3'] {
            fsio::write_bytes(&root.join(format!("library/materials/{}/a.png", key(set))), b"map").unwrap();
        }
        // Something in the third set the catalog knows nothing about.
        fsio::write_bytes(&root.join(format!("library/materials/{}/stray.txt", key('3'))), b"mine").unwrap();
        // The shared resources folder and a staging folder sit beside the sets.
        fsio::write_bytes(&root.join("library/materials/_resources/shared.png"), b"shared").unwrap();
        std::fs::create_dir_all(root.join("library/_import_left")).unwrap();
        let catalog = super::super::catalog_build::build_catalog(root.to_str().unwrap(), crate::domain::seed::parse_seed_entries(&manifest).unwrap()).unwrap();
        let cache = folder.join("cache.json");
        Store::local(&root, &cache).save_catalog(&catalog).unwrap();

        delete_from_library(&root, &cache, &["one".into(), "three".into()]).unwrap();
        let materials = root.join("library/materials");
        assert!(!materials.join(key('1')).exists(), "its only file is gone, so is the folder");
        assert!(materials.join(key('2')).join("a.png").exists(), "another item's set is untouched");
        assert!(!materials.join(key('3')).join("a.png").exists());
        assert!(materials.join(key('3')).join("stray.txt").exists(), "a folder with a file of the user's stays");
        assert!(materials.is_dir() && materials.join("_resources/shared.png").exists());
        assert!(root.join("library/_import_left").is_dir());

        delete_from_library(&root, &cache, &["two".into()]).unwrap();
        assert!(!materials.join(key('2')).exists());
        assert!(materials.is_dir(), "the type folder stays");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn only_the_newest_delete_copies_are_kept_in_both_places() {
        let folder = std::env::temp_dir().join(format!("toybox-delete-copies-{}", std::process::id()));
        let root = folder.join("library-root");
        std::fs::create_dir_all(root.join("_library_config")).unwrap();
        let count = 3 + crate::infra::retention::KEEP_DELETE_CATALOGS;
        let entry = |id: &str| json!({"id":id,"name":id,"kind":"material","pack":"p","category":"stone","maps":[{"role":"albedo","file":"m.png"}]});
        let ids: Vec<String> = (0..count).map(|n| format!("item{n}")).collect();
        let manifest = json!({"schemaVersion":1,"assets":ids.iter().map(|id| entry(id)).collect::<Vec<_>>()});
        store::write_manifest(&root, &manifest).unwrap();
        for id in &ids {
            fsio::write_bytes(&root.join(format!("library/p/materials/{id}/m.png")), id.as_bytes()).unwrap();
        }
        let catalog = super::super::catalog_build::build_catalog(root.to_str().unwrap(), crate::domain::seed::parse_seed_entries(&manifest).unwrap()).unwrap();
        let cache = folder.join("cache/catalog.json");
        Store::local(&root, &cache).save_catalog(&catalog).unwrap();
        // Unrelated files that look a little alike are not copies.
        fsio::write_bytes(&folder.join("cache/catalog-before-delete-notes.txt"), b"keep").unwrap();
        for id in &ids {
            delete_from_library(&root, &cache, &[id.clone()]).unwrap();
        }
        let copies = |dir: &Path| std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("catalog-before-delete-") && e.file_name().to_string_lossy().ends_with(".json")).count();
        assert_eq!(copies(&root.join("_library_config")), crate::infra::retention::KEEP_DELETE_CATALOGS);
        assert_eq!(copies(&folder.join("cache")), crate::infra::retention::KEEP_DELETE_CATALOGS);
        assert!(folder.join("cache/catalog-before-delete-notes.txt").exists());
        assert!(store::manifest_exists(&root) && Store::local(&root, &cache).catalog_exists());
        std::fs::remove_dir_all(folder).unwrap();
    }
}
