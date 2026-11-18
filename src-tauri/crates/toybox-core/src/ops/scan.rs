//! Building (or rebuilding) a library's catalog from Toybox's native
//! `catalog.json` source, resolving each asset's full fileset by reading its
//! glTF. Thin orchestration: infra-read -> domain-transform -> infra-write.

use crate::domain::catalog_model::Catalog;
use crate::domain::{merge, seed};
use crate::error::AppResult;
use crate::infra::registry::{self, LibraryHandle};

use super::catalog::{load_catalog_inner, save_catalog_inner};
use super::catalog_build::build_catalog;

/// Rebuild the derived catalog of one library from its manifest, keeping the
/// user's metadata from the cache already on disk, and save the result.
pub fn scan_library_catalog(handle: &LibraryHandle) -> AppResult<Catalog> {
    // A rescan rebuilds derived facts, while user metadata survives in this library.
    let prior = load_catalog_inner(handle)?.filter(|c| registry::same_root(&c.library_root, &handle.root_str()));

    let fresh = build_catalog_from_seed(&handle.root_str())?;

    let result = match prior {
        Some(prior) => merge::merge_preserving_user(&prior, fresh),
        None => fresh,
    };

    save_catalog_inner(handle, &result)?;
    let mut result = result;
    super::content_hash::fill_from_cache(handle, &mut result);
    Ok(result)
}

/// Read the manifest and assemble the catalog from its entries (textures and
/// animation resolved per asset). Produces default `user`/`thumb` metadata.
pub fn build_catalog_from_seed(root: &str) -> AppResult<Catalog> {
    let (manifest, revision) = crate::infra::store::read_manifest_with_revision(std::path::Path::new(root))?;
    let seed_doc = crate::infra::animation_import::prepare(root,manifest.clone())?;
    let entries = seed::parse_seed_entries(&seed_doc)?;
    let catalog = build_catalog(root, entries)?;
    crate::infra::animation_import::commit(root,&manifest,revision,&seed_doc)?;
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use crate::domain::paths;

    /// The maintainer's on-disk library, for the `--ignored` integration tests
    /// only. The app itself no longer hardcodes a root (it's user-chosen at
    /// runtime); these dev tests still need a concrete tree to seed from.
    const REAL_LIB: &str = r"D:\code\games\assets";

    /// Full seed against the real on-disk library. Asserts the verified counts
    /// from the plan. Run with `cargo test -- --ignored` on a machine that has
    /// the library at `REAL_LIB`.
    #[test]
    #[ignore = "requires the real asset library on disk"]
    fn seeds_real_library() {
        let catalog = build_catalog_from_seed(REAL_LIB).expect("seed");
        // 1535 original + 665 from the ingested Polygon SciFi Space pack.
        assert_eq!(catalog.assets.len(), 2200, "asset count");

        let packs: BTreeSet<_> = catalog.assets.iter().map(|a| a.pack.as_str()).collect();
        assert_eq!(
            packs,
            BTreeSet::from([
                "polygon_city",
                "polygon_prototype",
                "polygon_scifi",
                "polygon_scifi_space",
                "simple_military"
            ])
        );

        let categories: BTreeSet<_> =
            catalog.assets.iter().map(|a| a.category.as_str()).collect();
        assert_eq!(categories.len(), 11, "category count");

        // The new pack resolved its files and a rigged character is present.
        let scifi_space = catalog
            .assets
            .iter()
            .filter(|a| a.pack == "polygon_scifi_space")
            .count();
        assert_eq!(scifi_space, 665, "scifi space pack asset count");
        let crew = catalog
            .assets
            .iter()
            .find(|a| a.name == "SK_Chr_Crew_Male_01")
            .expect("crew character present");
        let crew_gltf = paths::abs_under_root(&catalog.library_root, &crew.fileset().unwrap().gltf);
        assert!(crew_gltf.exists(), "crew gltf resolves");

        // Spot-check the verification asset and its two textures resolve.
        let a = catalog
            .assets
            .iter()
            .find(|a| a.name == "SM_Bld_Apartment_01")
            .expect("apartment asset present");
        let fileset = a.fileset().unwrap();
        assert_eq!(fileset.textures.len(), 2, "apartment textures");
        for tex in &fileset.textures {
            let abs = paths::abs_under_root(&catalog.library_root, tex);
            assert!(abs.exists(), "texture missing: {abs:?}");
        }
        let gltf_abs = paths::abs_under_root(&catalog.library_root, &fileset.gltf);
        assert!(gltf_abs.exists(), "gltf missing");

        // The lone TGA asset must resolve too.
        let tga = catalog
            .assets
            .iter()
            .find(|a| a.name == "SM_Env_Skyline_01")
            .expect("skyline asset present");
        assert!(
            tga.fileset().unwrap().textures.iter().any(|t| t.ends_with(".tga")),
            "skyline should reference a .tga"
        );

        // Animation extraction: 82 original + 6 SciFi Space "BR" characters that
        // ship a real (non-empty) take.
        let animated = catalog
            .assets
            .iter()
            .filter(|a| a.animation().is_ok_and(|m| m.clip_count > 0))
            .count();
        assert_eq!(animated, 88, "assets with real animation clips");
        // A known animated character carries at least one named clip.
        let chr = catalog
            .assets
            .iter()
            .find(|a| a.name == "Character_Alien_Male_01")
            .expect("alien character present");
        assert!(chr.animation().unwrap().clip_count >= 1, "alien should be animated");
    }
}
