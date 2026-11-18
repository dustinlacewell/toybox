//! Offline indexing for bulk imports; never starts a window or runs plugins.
use std::collections::BTreeMap;
use std::path::Path;
use crate::domain::{merge, seed};

/// Migration only: no reindex, filename inference or animation extraction.
pub fn migrate_library(root:&Path,cache:&Path)->Result<serde_json::Value,String>{
    (||->crate::error::AppResult<serde_json::Value>{
        crate::infra::migrations::run(root,cache)?;
        let manifest=crate::infra::store::read_manifest(root)?;
        let entries=seed::parse_seed_entries(&manifest)?;
        let validated_cache_items=crate::infra::store::Store::local(root,cache).load_catalog()?.map(|catalog|catalog.assets.len());
        let mut counts=BTreeMap::new();for entry in &entries{
            let kind=match &entry.content{seed::SeedContent::Model{..}=>"model",seed::SeedContent::Material{..}=>"material",seed::SeedContent::AnimationSource{..}=>"animation",seed::SeedContent::Audio{..}=>"audio",seed::SeedContent::Plugin{type_id,..}=>type_id.as_str()};
            *counts.entry(kind.to_string()).or_insert(0usize)+=1;
        }
        Ok(serde_json::json!({"storageVersion":manifest["storageVersion"],"sourceEntries":entries.len(),"validatedCachedItems":validated_cache_items,"counts":counts,"backupDirectory":root.join("_library_config/migration-backups")}))
    })().map_err(|e|e.to_string())
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReindexReport { pub source_entries: usize, pub total_assets: usize, pub counts: BTreeMap<String, usize>, pub backup: Option<String> }

pub fn reindex(root: &Path, cache: &Path) -> Result<ReindexReport, String> {
    reindex_inner(root, cache).map_err(|error| error.to_string())
}

fn reindex_inner(root: &Path, cache: &Path) -> crate::error::AppResult<ReindexReport> {
    crate::infra::migrations::run(root,cache)?;
    let (source, revision) = crate::infra::store::read_manifest_with_revision(root)?;
    let doc = crate::infra::animation_import::prepare(&root.to_string_lossy(),source.clone())?;
    let entries = seed::parse_seed_entries(&doc)?;
    let source_entries = entries.len();
    let fresh = crate::ops::catalog_build::build_catalog(&root.to_string_lossy(), entries)?;
    if crate::infra::store::manifest_revision(root)? != revision { return Err(crate::error::AppError::msg("Source manifest changed during indexing; retry when import is idle.")); }
    crate::infra::animation_import::commit(&root.to_string_lossy(),&source,revision,&doc)?;
    // Read user metadata only after the potentially expensive source scan.
    let store = crate::infra::store::Store::local(root, cache);
    let old = store.load_catalog()?;
    let catalog = match old.as_ref() {
        Some(prior) => merge::merge_preserving_user(prior, fresh),
        None => fresh,
    };
    let mut counts = BTreeMap::new();
    for asset in &catalog.assets {
        *counts.entry(asset.content.type_id().to_string()).or_insert(0) += 1;
    }
    let parent = cache.parent().ok_or_else(|| crate::error::AppError::msg("Cache must have a parent folder"))?;
    std::fs::create_dir_all(parent)?;
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let backup = if let Some(prior) = &old {
        let path = parent.join(format!("catalog-before-reindex-{timestamp}.json"));
        std::fs::write(&path, serde_json::to_vec_pretty(prior)?)?; Some(path.to_string_lossy().to_string())
    } else { None };
    store.save_catalog(&catalog)?;
    Ok(ReindexReport { source_entries, total_assets: catalog.assets.len(), counts, backup })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn offline_reindex_backs_up_cache_and_preserves_user_metadata() {
        let folder = std::env::temp_dir().join(format!("toybox-reindex-test-{}", std::process::id()));
        let root = folder.join("library-root"); let config = root.join("_library_config");
        std::fs::create_dir_all(&config).unwrap();
        let source = json!({"assets":[{"kind":"material","id":"stone","name":"Stone","pack":"p","category":"stone","maps":[{"role":"albedo","file":"a.png"}]}]});
        std::fs::write(config.join("catalog.json"), serde_json::to_vec(&source).unwrap()).unwrap();
        let material = root.join("library/p/materials/Stone"); std::fs::create_dir_all(&material).unwrap(); std::fs::write(material.join("a.png"), b"image").unwrap();
        let cache = folder.join("catalog.json");
        let prior = json!({"schemaVersion":3,"libraryRoot":root.to_string_lossy(),"assets":[{"kind":"material","id":"stone","name":"Stone","fileName":"Stone","relPath":"library/p/materials/Stone","pack":"p","category":"stone","material":{"maps":[{"role":"albedo","file":"library/p/materials/Stone/a.png"}]},"user":{"favorite":true,"tags":["tag"],"collections":["Spygame"]},"thumb":{"state":"ready"}}]});
        std::fs::write(&cache, serde_json::to_vec(&prior).unwrap()).unwrap();
        let report = reindex(&root, &cache).unwrap(); assert_eq!(report.total_assets, 1); assert_eq!(report.counts["material"], 1);
        assert!(Path::new(report.backup.as_ref().unwrap()).is_file());
        let result: Value = serde_json::to_value(crate::infra::store::Store::local(&root, &cache).load_catalog().unwrap().unwrap()).unwrap();
        let user=&result["assets"][0]["user"];
        assert_eq!(user["favorite"],prior["assets"][0]["user"]["favorite"]);
        assert_eq!(user["collections"],prior["assets"][0]["user"]["collections"]);
        assert!(user["tags"].as_array().unwrap().contains(&json!("tag")));
        assert_eq!(user["provenance"]["source"],"p");
        assert_eq!(result["schemaVersion"], crate::config::SCHEMA_VERSION);
        std::fs::remove_dir_all(folder).unwrap();
    }
}
