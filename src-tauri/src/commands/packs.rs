//! Compatibility source metadata API. Managed libraries read the manifest;
//! only libraries awaiting migration consult legacy pack.json directories.

use std::path::Path;

use tauri::AppHandle;

use crate::domain::pack_meta::PackMeta;
use crate::error::AppResult;
use crate::infra::{fsio, library};

#[tauri::command]
pub fn load_packs(app: AppHandle) -> AppResult<Vec<PackMeta>> {
    let root=library::resolve(&app)?;
    let manifest:serde_json::Value=serde_json::from_slice(&fsio::read_bytes(&root.join(crate::config::SEED_REL))?)?;
    if manifest["storageVersion"]==1 {
        let mut sources=std::collections::BTreeMap::new();
        for entry in manifest["assets"].as_array().into_iter().flatten(){
            let slug=entry["pack"].as_str().unwrap_or("imported");
            sources.entry(slug.to_string()).or_insert_with(||PackMeta{slug:slug.into(),name:entry["provenance"]["source"].as_str().unwrap_or(slug).into(),color:"7ed321ff".into()});
        }
        return Ok(sources.into_values().collect());
    }
    read_packs_under(&root.join("library"))
}

/// Read every `<pack>/pack.json` directly under `library`. An absent directory
/// yields an empty list (a freshly-pointed library may have no packs yet).
fn read_packs_under(library: &Path) -> AppResult<Vec<PackMeta>> {
    let mut packs = Vec::new();
    if !library.exists() {
        return Ok(packs);
    }
    for entry in std::fs::read_dir(library)? {
        let dir = entry?.path();
        if !dir.is_dir() {
            continue;
        }
        let meta_path = dir.join("pack.json");
        if !fsio::exists(&meta_path) {
            continue;
        }
        let text = fsio::read_text(&meta_path)?;
        let meta: PackMeta = serde_json::from_str(&text)?;
        packs.push(meta);
    }
    packs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const REAL_LIB: &str = r"D:\code\games\assets";

    #[test]
    #[ignore = "requires the real asset library on disk"]
    fn loads_all_pack_metadata() {
        let packs = read_packs_under(&Path::new(REAL_LIB).join("library")).expect("load packs");
        let by_slug: BTreeMap<_, _> = packs.iter().map(|p| (p.slug.as_str(), p)).collect();

        // All five packs self-describe via pack.json.
        for slug in [
            "polygon_city",
            "polygon_prototype",
            "polygon_scifi",
            "polygon_scifi_space",
            "simple_military",
        ] {
            assert!(by_slug.contains_key(slug), "missing pack.json for {slug}");
        }
        assert_eq!(by_slug["polygon_scifi_space"].name, "Polygon SciFi Space");
        assert_eq!(by_slug["polygon_scifi_space"].color, "7b61ffff");
    }
}
