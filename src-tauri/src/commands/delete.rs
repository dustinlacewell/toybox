//! Delete library registrations and exclusively owned files.
use crate::{
    config,
    domain::catalog_model::Catalog,
    error::{AppError, AppResult},
    infra::{appdata, fsio, library},
};
use serde_json::Value;
use std::{collections::HashSet, path::Path};
use tauri::AppHandle;

fn remove_selected(catalog: &mut Catalog, manifest: &mut Value, ids: &[String]) -> AppResult<()> {
    let selected: HashSet<_> = ids.iter().map(String::as_str).collect();
    if selected.is_empty() {
        return Err(AppError::msg("Select items to delete"));
    }
    let mut removed = HashSet::new();
    for id in &selected {
        let asset = catalog
            .assets
            .iter()
            .find(|asset| asset.id == *id)
            .ok_or_else(|| {
                AppError::msg(format!("Selected item is no longer in the library: {id}"))
            })?;
        removed.insert(asset.id.clone());
        removed.extend(asset.variants.iter().map(|variant| variant.id.clone()));
    }
    let entries = manifest
        .get_mut("assets")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| AppError::msg("Library manifest has no assets"))?;
    if selected
        .iter()
        .any(|id| !entries.iter().any(|entry| entry["id"].as_str() == Some(id)))
    {
        return Err(AppError::msg(
            "Library registrations changed. Rescan before deleting these items.",
        ));
    }
    entries.retain(|entry| !entry["id"].as_str().is_some_and(|id| selected.contains(id)));
    catalog
        .assets
        .retain(|asset| !selected.contains(asset.id.as_str()));
    for asset in &mut catalog.assets {
        if asset
            .user
            .preview_asset_id
            .as_ref()
            .is_some_and(|id| removed.contains(id))
        {
            asset.user.preview_asset_id = None;
        }
        for variant in &mut asset.variants {
            if variant
                .user
                .preview_asset_id
                .as_ref()
                .is_some_and(|id| removed.contains(id))
            {
                variant.user.preview_asset_id = None;
            }
        }
    }
    Ok(())
}

pub fn delete_from_library(root: &Path, cache: &Path, ids: &[String]) -> AppResult<Catalog> {
    let manifest_path = root.join(config::SEED_REL);
    let old_manifest = fsio::read_bytes(&manifest_path)?;
    let old_cache = fsio::read_bytes(cache)?;
    let mut manifest: Value = serde_json::from_slice(&old_manifest)?;
    let mut catalog: Catalog = serde_json::from_slice(&old_cache)?;
    if Path::new(&catalog.library_root).canonicalize()? != root.canonicalize()? {
        return Err(AppError::msg(
            "The selected library changed. Reload before deleting items.",
        ));
    }
    let mut delete_files = super::delete_files::exclusive_files(root, &catalog, ids)?;
    delete_files.extend(super::delete_files::exclusive_thumbnails(
        cache, &catalog, ids,
    )?);
    remove_selected(&mut catalog, &mut manifest, ids)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let manifest_temp = manifest_path.with_file_name(format!("catalog-delete-{stamp}.tmp"));
    let cache_temp = cache.with_file_name(format!("catalog-delete-{stamp}.tmp"));
    let result = (|| {
        fsio::write_bytes(
            &manifest_path.with_file_name(format!("catalog-before-delete-{stamp}.json")),
            &old_manifest,
        )?;
        fsio::write_bytes(
            &cache.with_file_name(format!("catalog-before-delete-{stamp}.json")),
            &old_cache,
        )?;
        fsio::write_bytes(&manifest_temp, &serde_json::to_vec_pretty(&manifest)?)?;
        fsio::write_bytes(&cache_temp, &serde_json::to_vec_pretty(&catalog)?)?;
        if fsio::read_bytes(&manifest_path)? != old_manifest
            || fsio::read_bytes(cache)? != old_cache
        {
            return Err(AppError::msg(
                "Library changed while deleting. Retry after other library operations finish.",
            ));
        }
        let staged = super::delete_files::StagedFiles::stage(delete_files, stamp)?;
        std::fs::rename(&manifest_temp, &manifest_path)
            .map_err(|error| AppError::file("Update library manifest", &manifest_path, error))?;
        if let Err(error) = std::fs::rename(&cache_temp, cache) {
            // Restore the authoritative manifest if the cache cannot commit.
            fsio::write_bytes(&manifest_temp, &old_manifest)?;
            std::fs::rename(&manifest_temp, &manifest_path).map_err(|error| {
                AppError::file("Restore library manifest", &manifest_path, error)
            })?;
            return Err(AppError::file("Update library cache", cache, error));
        }
        staged.erase()?;
        Ok(catalog)
    })();
    let _ = fsio::remove_file_if_exists(&manifest_temp);
    let _ = fsio::remove_file_if_exists(&cache_temp);
    result
}

#[tauri::command]
pub async fn delete_assets(app: AppHandle, asset_ids: Vec<String>) -> AppResult<Catalog> {
    let root = library::resolve(&app)?;
    let cache = appdata::catalog_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || delete_from_library(&root, &cache, &asset_ids))
        .await
        .map_err(|error| AppError::msg(format!("Delete worker failed: {error}")))?
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
        let source = root.join(config::SEED_REL);
        fsio::write_bytes(&source, &serde_json::to_vec(&manifest).unwrap()).unwrap();
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
        let cache_bytes = serde_json::to_vec(&catalog).unwrap();
        fsio::write_bytes(&cache, &cache_bytes).unwrap();
        assert!(delete_from_library(&root, &cache, &["absent".into()]).is_err());
        assert_eq!(fsio::read_bytes(&cache).unwrap(), cache_bytes);
        let result = delete_from_library(&root, &cache, &["one".into()]).unwrap();
        assert_eq!(result.assets.len(), 1);
        assert_eq!(result.assets[0].id, "two");
        assert!(result.assets[0].user.favorite);
        assert!(result.assets[0].user.preview_asset_id.is_none());
        let updated: Value = serde_json::from_slice(&fsio::read_bytes(&source).unwrap()).unwrap();
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
            let before_manifest = fsio::read_bytes(&source).unwrap();
            let before_cache = fsio::read_bytes(&cache).unwrap();
            let locked = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(&cache)
                .unwrap();
            assert!(delete_from_library(&root, &cache, &["two".into()]).is_err());
            assert_eq!(fsio::read_bytes(&source).unwrap(), before_manifest);
            assert_eq!(fsio::read_bytes(&cache).unwrap(), before_cache);
            assert_eq!(
                fsio::read_bytes(&root.join("library/p/materials/two/shared.png")).unwrap(),
                b"source"
            );
            assert!(folder.join("thumbs/two.png").exists());
            drop(locked);
        }
        std::fs::remove_dir_all(folder).unwrap();
    }
}
