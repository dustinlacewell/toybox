//! `scan_library`: build (or rebuild) the catalog from the Godot curation file,
//! resolving each asset's full fileset by reading its glTF. Thin orchestration:
//! infra-read -> domain-transform -> infra-write.

use std::path::Path;

use tauri::AppHandle;

use crate::config;
use crate::domain::catalog_model::{AnimationMeta, Catalog};
use crate::domain::{gltf_parse, merge, paths, seed};
use crate::error::{AppError, AppResult};
use crate::infra::fsio;

use super::catalog::{load_catalog_inner, save_catalog_inner};

#[tauri::command]
pub async fn scan_library(app: AppHandle, force_reseed: bool) -> AppResult<Catalog> {
    let prior = if force_reseed { None } else { load_catalog_inner(&app)? };

    let fresh = build_catalog_from_seed()?;

    let result = match prior {
        Some(prior) => merge::merge_preserving_user(&prior, fresh),
        None => fresh,
    };

    save_catalog_inner(&app, &result)?;
    Ok(result)
}

/// Read `asset_library.json`, then resolve each asset's textures by parsing its
/// glTF. Produces a catalog with default `user`/`thumb` metadata.
fn build_catalog_from_seed() -> AppResult<Catalog> {
    let root = config::LIBRARY_ROOT;
    let seed_abs = paths::abs_under_root(root, config::SEED_REL);
    let seed_text = fsio::read_text(&seed_abs)?;
    let seed_doc = serde_json::from_str(&seed_text)?;

    let entries = seed::parse_seed_entries(&seed_doc)?;

    let mut assets = Vec::with_capacity(entries.len());
    for entry in entries {
        let facets = read_asset_facets(root, &entry.gltf_rel)?;
        assets.push(seed::build_asset(entry, facets.textures, facets.animation));
    }

    Ok(Catalog {
        schema_version: config::SCHEMA_VERSION,
        library_root: root.to_string(),
        assets,
    })
}

/// Facts read from one glTF: its texture set and animation clips.
struct AssetFacets {
    textures: Vec<String>,
    animation: AnimationMeta,
}

/// Parse one glTF once, returning its unique deduped texture paths (library-
/// relative) and its real animation clips.
fn read_asset_facets(root: &str, gltf_rel: &str) -> AppResult<AssetFacets> {
    let gltf_abs = paths::abs_under_root(root, gltf_rel);
    let text = fsio::read_text(&gltf_abs)?;
    let doc = gltf_parse::parse(&text)?;

    let rel_dir = parent_rel(gltf_rel);
    let mut textures: Vec<String> = Vec::new();
    for image in gltf_parse::image_uris(&doc) {
        let tex_rel = paths::resolve_uri_rel(&rel_dir, &image.uri);
        if !textures.contains(&tex_rel) {
            textures.push(tex_rel);
        }
    }

    let clip_names = gltf_parse::animation_clip_names(&doc);
    let animation = AnimationMeta {
        clip_count: clip_names.len() as u32,
        clip_names,
    };

    Ok(AssetFacets { textures, animation })
}

/// The directory portion of a library-relative path.
fn parent_rel(rel: &str) -> String {
    Path::new(rel)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .ok_or(())
        .unwrap_or_default()
}

/// Surface a clear error if the library root is missing entirely.
#[allow(dead_code)]
fn require_root_exists() -> AppResult<()> {
    if !Path::new(config::LIBRARY_ROOT).exists() {
        return Err(AppError::msg(format!(
            "library root not found: {}",
            config::LIBRARY_ROOT
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Full seed against the real on-disk library. Asserts the verified counts
    /// from the plan. Run with `cargo test -- --ignored` on a machine that has
    /// the library at `config::LIBRARY_ROOT`.
    #[test]
    #[ignore = "requires the real asset library on disk"]
    fn seeds_real_library() {
        let catalog = build_catalog_from_seed().expect("seed");
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
        let crew_gltf = paths::abs_under_root(&catalog.library_root, &crew.fileset.gltf);
        assert!(crew_gltf.exists(), "crew gltf resolves");

        // Spot-check the verification asset and its two textures resolve.
        let a = catalog
            .assets
            .iter()
            .find(|a| a.name == "SM_Bld_Apartment_01")
            .expect("apartment asset present");
        assert_eq!(a.fileset.textures.len(), 2, "apartment textures");
        for tex in &a.fileset.textures {
            let abs = paths::abs_under_root(&catalog.library_root, tex);
            assert!(abs.exists(), "texture missing: {abs:?}");
        }
        let gltf_abs = paths::abs_under_root(&catalog.library_root, &a.fileset.gltf);
        assert!(gltf_abs.exists(), "gltf missing");

        // The lone TGA asset must resolve too.
        let tga = catalog
            .assets
            .iter()
            .find(|a| a.name == "SM_Env_Skyline_01")
            .expect("skyline asset present");
        assert!(
            tga.fileset.textures.iter().any(|t| t.ends_with(".tga")),
            "skyline should reference a .tga"
        );

        // Animation extraction: 82 original + 6 SciFi Space "BR" characters that
        // ship a real (non-empty) take.
        let animated = catalog
            .assets
            .iter()
            .filter(|a| a.animation.clip_count > 0)
            .count();
        assert_eq!(animated, 88, "assets with real animation clips");
        // A known animated character carries at least one named clip.
        let chr = catalog
            .assets
            .iter()
            .find(|a| a.name == "Character_Alien_Male_01")
            .expect("alien character present");
        assert!(chr.animation.clip_count >= 1, "alien should be animated");
    }
}
