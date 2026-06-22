//! Shared catalog-assembly helpers for the seeding commands. Both `scan_library`
//! (seeds from the native `catalog.json`) and `merge_seed_entries` (seeds from
//! plugin-supplied entries) turn `SeedEntry`s into full `Asset`s the same way:
//! read each glTF for its textures/animation, then `seed::build_asset`. Keeping
//! this in one place means file-seeding and import-seeding can never diverge.

use std::path::Path;

use crate::config;
use crate::domain::catalog_model::{AnimationMeta, Catalog};
use crate::domain::seed::{self, SeedEntry};
use crate::domain::{gltf_parse, paths};
use crate::error::AppResult;
use crate::infra::fsio;

/// Assemble a full catalog from already-parsed seed entries, resolving each
/// asset's textures/animation by reading its glTF. Produces default
/// `user`/`thumb` metadata; the caller merges to preserve prior user metadata.
/// `root` is the live library root the assets are read from and stamped with.
pub fn build_catalog(root: &str, entries: Vec<SeedEntry>) -> AppResult<Catalog> {
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
pub struct AssetFacets {
    pub textures: Vec<String>,
    pub animation: AnimationMeta,
}

/// Parse one glTF once, returning its unique deduped texture paths (library-
/// relative) and its real animation clips.
pub fn read_asset_facets(root: &str, gltf_rel: &str) -> AppResult<AssetFacets> {
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
