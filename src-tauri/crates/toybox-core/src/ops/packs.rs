//! Source metadata API. Managed libraries read the manifest; only libraries
//! awaiting migration consult legacy pack.json directories.

use std::path::Path;

use crate::domain::pack_meta::PackMeta;
use crate::error::AppResult;
use crate::infra::fsio;

/// The packs of a library: read from the folder for a local library, derived from
/// the cached catalog for a remote one (whose manifest stays on the server).
pub fn load_packs_for(handle: &crate::infra::registry::LibraryHandle) -> AppResult<Vec<PackMeta>> {
    if !handle.is_remote() {
        return load_packs(&handle.root);
    }
    let mut seen = std::collections::BTreeMap::new();
    super::catalog::open_library(handle)?;
    for pack in crate::infra::store::Store::for_handle(handle).catalog_packs()? {
        seen.entry(pack.clone()).or_insert_with(|| PackMeta { slug: pack.clone(), name: pack, color: "7ed321ff".into() });
    }
    Ok(seen.into_values().collect())
}

/// The packs (sources) of the library at `root`.
pub fn load_packs(root:&Path)->AppResult<Vec<PackMeta>> {
    let store=crate::infra::store::Store::local(root,&root.join("_library_config/none"));
    if !crate::infra::store::manifest_exists(root) {
        return Err(crate::error::AppError::msg(format!("not a Toybox library: no manifest in {}", root.display())));
    }
    if store.meta("storageVersion").as_deref()==Some("1") {
        let mut sources=std::collections::BTreeMap::new();
        for (pack,entry) in store.pack_samples()?{
            let slug=if pack.is_empty(){"imported".to_string()}else{pack};
            sources.entry(slug.clone()).or_insert_with(||PackMeta{slug:slug.clone(),name:entry["provenance"]["source"].as_str().unwrap_or(&slug).into(),color:"7ed321ff".into()});
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
