//! Shared catalog-assembly helpers for the seeding commands. Both `scan_library`
//! (seeds from the native `catalog.json`) and `merge_seed_entries` (seeds from
//! plugin-supplied entries) turn `SeedEntry`s into full `Asset`s the same way:
//! read each glTF for its textures/animation, then `seed::build_asset`. Keeping
//! this in one place means file-seeding and import-seeding can never diverge.

use std::path::Path;

use crate::config;
use crate::domain::catalog_model::{AnimationMeta, Asset, AssetContent, Catalog};
use crate::domain::material::MaterialSet;
use crate::domain::seed::{self, SeedContent, SeedEntry};
use crate::domain::{gltf_parse, gltf_uri, paths};
use crate::error::{AppError, AppResult};
use crate::infra::fsio;

/// Assemble a full catalog from already-parsed seed entries. A model's
/// textures/animation are resolved by reading its glTF; a material's map files
/// are only checked for existence. Produces default `user`/`thumb` metadata;
/// the caller merges to preserve prior user metadata.
/// `root` is the live library root the assets are read from and stamped with.
pub fn build_catalog(root: &str, entries: Vec<SeedEntry>) -> AppResult<Catalog> {
    build_catalog_with(root, entries, &()).map(|(catalog, _)| catalog)
}

/// Rigs a library already stores. A rig described from a file that is equivalent
/// to a stored one binds to the stored one, as it would to an earlier rig of the
/// same build.
pub trait KnownRigs {
    fn equivalent_to(&self, rig: &crate::domain::rig::Rig) -> Option<crate::domain::rig::Rig>;
}

impl KnownRigs for () {
    fn equivalent_to(&self, _: &crate::domain::rig::Rig) -> Option<crate::domain::rig::Rig> {
        None
    }
}

impl KnownRigs for crate::infra::store::Store {
    fn equivalent_to(&self, rig: &crate::domain::rig::Rig) -> Option<crate::domain::rig::Rig> {
        self.find_equivalent_rig(rig).ok().flatten()
    }
}

/// [`build_catalog`] that also knows the rigs a library stores, and says which
/// entry (by position in `entries`) each asset of the result came from.
pub fn build_catalog_with(root: &str, entries: Vec<SeedEntry>, known: &dyn KnownRigs) -> AppResult<(Catalog, Vec<usize>)> {
    let mut assets = Vec::with_capacity(entries.len());
    let mut owners = Vec::with_capacity(entries.len());
    let mut seen_clips = std::collections::HashSet::new();
    let mut rigs: Vec<crate::domain::rig::Rig> = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        let animation_only = matches!(entry.content, SeedContent::AnimationSource { .. });
        let mut asset = build_one(root, entry)?;
        if let AssetContent::Model { fileset, .. } = &asset.content {
            let text = fsio::read_text(&paths::abs_under_root(root, &fileset.gltf))?;
            let doc = gltf_parse::parse(&text)?;
            let clips = if animation_only { crate::domain::animation::clips(&doc, &asset, fileset) } else { vec![] };
            if animation_only && clips.is_empty() { return Err(AppError::msg(format!("{} has no valid animation clips", asset.file_name))); }
            for mut clip in clips {
                if seen_clips.insert(clip.id.clone()) { register_rig(root, &mut clip, &mut rigs, known)?; assets.push(clip); owners.push(index); }
            }
        }
        if !animation_only { register_rig(root, &mut asset, &mut rigs, known)?; assets.push(asset); owners.push(index); }
    }
    Ok((Catalog {
        schema_version: config::SCHEMA_VERSION,
        library_root: root.to_string(),
        rigs,
        assets,
    }, owners))
}

fn register_rig(root: &str, asset: &mut Asset, registry: &mut Vec<crate::domain::rig::Rig>, known: &dyn KnownRigs) -> AppResult<()> {
    for variant in &mut asset.variants { register_rig(root,variant,registry,known)?; }
    let Ok(fileset) = asset.fileset() else { return Ok(()) };
    let doc = gltf_parse::parse(&fsio::read_text(&paths::abs_under_root(root,&fileset.gltf))?)?;
    if let Some((rig,mut binding)) = crate::domain::rig::describe(&doc) {
        if let Some(existing) = registry.iter().find(|existing| crate::domain::rig::equivalent(existing,&rig)) { binding.rig_id = existing.id.clone(); }
        else if let Some(stored) = known.equivalent_to(&rig) {
            binding.rig_id = stored.id.clone();
            if !registry.iter().any(|existing| existing.id == stored.id) { registry.push(stored); }
        }
        else if !registry.iter().any(|existing| existing.id == rig.id) { registry.push(rig); }
        if let AssetContent::Animation { clip, .. } = &mut asset.content {
            clip.rig = registry.iter().find(|rig| rig.id == binding.rig_id).map(|rig| format!("{} · {} nodes",rig.name,rig.nodes.len())).unwrap_or_default();
        }
        asset.rigs = vec![binding];
    }
    Ok(())
}

fn build_one(root: &str, mut entry: SeedEntry) -> AppResult<Asset> {
    let variants = std::mem::take(&mut entry.variants).into_iter()
        .map(|v| build_one(root, v)).collect::<AppResult<Vec<_>>>()?;
    if let SeedContent::AnimationSource { gltf_rel, bin_rel } = &entry.content {
        entry.content = SeedContent::Model { gltf_rel: gltf_rel.clone(), bin_rel: bin_rel.clone() };
    }
    let mut asset = match &entry.content {
        SeedContent::Audio {file} => {
            let audio=crate::infra::audio::probe(Path::new(root),file).map_err(|error|AppError::msg(format!("{file}: {error}")))?;
            Ok(seed::build_audio_asset(entry,audio))
        }
        SeedContent::Model { gltf_rel, .. } => {
            let facets = read_asset_facets(root, gltf_rel)?;
            seed::build_asset(entry, facets.textures, facets.animation)
        }
        SeedContent::Material { material } => {
            check_textures_exist(root, &entry.id, material)?;
            for file in entry.origin.iter().flat_map(|origin| &origin.files) {
                if !fsio::exists(&paths::abs_under_root(root, file)) {
                    return Err(AppError::msg(format!("asset {}: missing source file {file}", entry.id)));
                }
            }
            seed::build_material_asset(entry)
        }
        SeedContent::Plugin { files, .. } => {
            for file in files {
                if !fsio::exists(&paths::abs_under_root(root, file)) {
                    return Err(AppError::msg(format!("asset {}: missing file {file}", entry.id)));
                }
            }
            seed::build_plugin_asset(entry)
        }
        SeedContent::AnimationSource { .. } => unreachable!("animation source normalized above"),
    }?;
    asset.variants = variants;
    Ok(asset)
}

/// Error naming the asset and file for the first map file missing under `root`.
fn check_textures_exist(root: &str, id: &str, material: &MaterialSet) -> AppResult<()> {
    for file in material.textures() {
        if !fsio::exists(&paths::abs_under_root(root, &file)) {
            return Err(AppError::msg(format!("asset {id}: missing material file {file}")));
        }
    }
    Ok(())
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
        // An embedded `data:` image is part of the glTF, not a texture file.
        let Some(uri) = gltf_uri::external_file(&image.uri) else { continue };
        let tex_rel = paths::resolve_uri_rel(&rel_dir, uri);
        if !textures.contains(&tex_rel) {
            textures.push(tex_rel);
        }
    }

    let clip_names = Vec::new();
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn animation_sources_produce_one_record_per_clip_and_exports_select_exact_clip() {
        let root = std::env::temp_dir().join(format!("toybox-animation-catalog-{}", std::process::id()));
        let folder = root.join("library/p/animations"); std::fs::create_dir_all(&folder).unwrap();
        let gltf = json!({"asset":{"version":"2.0"},"nodes":[{"name":"Root"}],"accessors":[{"max":[2.0]}],"animations":[{"channels":[]},{"name":"Walk","samplers":[{"input":0,"output":1}],"channels":[{"sampler":0,"target":{"node":0,"path":"translation"}}]},{"name":"Run","samplers":[{"input":0,"output":1}],"channels":[{"sampler":0,"target":{"node":0,"path":"translation"}}]}]});
        std::fs::write(folder.join("Hero.gltf"), serde_json::to_vec(&gltf).unwrap()).unwrap();
        let doc = json!({"assets":[{"kind":"animation","id":"source","pack":"p","category":"animations","file":"Hero.gltf"}]});
        let result = build_catalog(&root.to_string_lossy(), seed::parse_seed_entries(&doc).unwrap()).unwrap();
        assert_eq!(result.assets.len(), 2); assert!(result.assets.iter().all(|a| matches!(a.content, AssetContent::Animation { .. })));
        let AssetContent::Animation { clip, .. } = &result.assets[1].content else { panic!("animation expected") };
        assert_eq!(clip.index, 2); assert_eq!(clip.name, "Run");
        let selected = super::super::export_util::read_gltf(&root.to_string_lossy(), &result.assets[1]).unwrap();
        assert_eq!(selected["animations"].as_array().unwrap().len(), 1); assert_eq!(selected["animations"][0]["name"], "Run");
        // Model records no longer expose embedded clips. Import extraction
        // registers independent animation entries before catalog assembly.
        let model = json!({"assets":[{"id":"source","pack":"p","category":"animations","file":"Hero.gltf"}]});
        let with_model = build_catalog(&root.to_string_lossy(), seed::parse_seed_entries(&model).unwrap()).unwrap();
        assert_eq!(with_model.assets.len(), 1);
        assert_eq!(with_model.assets[0].animation().unwrap().clip_count, 0);
        assert!(super::super::export_util::read_gltf(&root.to_string_lossy(), &with_model.assets[0]).unwrap().get("animations").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builds_one_material_with_distinct_size_filesets_and_checks_every_size() {
        let root = std::env::temp_dir().join(format!("toybox-variant-catalog-{}", std::process::id()));
        for name in ["Wood_4K", "Wood_2K"] {
            let dir = root.join(format!("library/p/materials/{name}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("albedo.png"), b"image").unwrap();
        }
        let doc = json!({ "assets": [{ "kind":"material", "id":"wood", "pack":"p", "category":"wood", "name":"Wood_4K", "maps":[{"role":"albedo","file":"albedo.png"}],
            "variants":[{"kind":"material","id":"wood-2k","pack":"p","category":"wood","name":"Wood_2K","maps":[{"role":"albedo","file":"albedo.png"}]}] }] });
        let catalog = build_catalog(&root.to_string_lossy(), seed::parse_seed_entries(&doc).unwrap()).unwrap();
        assert_eq!(catalog.assets.len(), 1);
        assert_eq!(catalog.assets[0].variants.len(), 1);
        assert_eq!(catalog.assets[0].variants[0].rel_path, "library/p/materials/Wood_2K");
        std::fs::remove_file(root.join("library/p/materials/Wood_2K/albedo.png")).unwrap();
        assert!(build_catalog(&root.to_string_lossy(), seed::parse_seed_entries(&doc).unwrap()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
