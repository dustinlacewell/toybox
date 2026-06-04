//! `load_packs`: read every pack's `pack.json` from the library. Packs are
//! self-describing directories — their identity (name, color) lives with them,
//! not in the asset catalog or the Godot curation file.

use std::path::Path;

use crate::config;
use crate::domain::pack_meta::PackMeta;
use crate::error::AppResult;
use crate::infra::fsio;

#[tauri::command]
pub fn load_packs() -> AppResult<Vec<PackMeta>> {
    let library = Path::new(config::LIBRARY_ROOT).join("library");
    let mut packs = Vec::new();
    if !library.exists() {
        return Ok(packs);
    }
    for entry in std::fs::read_dir(&library)? {
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

    #[test]
    #[ignore = "requires the real asset library on disk"]
    fn loads_all_pack_metadata() {
        let packs = load_packs().expect("load packs");
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
