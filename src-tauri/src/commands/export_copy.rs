//! `export_copy`: copy selected assets as self-contained loose glTF filesets
//! into a target directory, rewriting texture/buffer URIs so each export stands
//! alone. Textures are deduped across the batch; flatten-mode glTF basename
//! collisions are guarded by namespacing the stem with the pack.

use std::collections::HashSet;
use std::path::Path;

use tauri::AppHandle;

use crate::domain::catalog_model::{Asset, AssetContent};
use crate::domain::export_copy_plan::plan_copy;
use crate::domain::export_model::{ExportCopyReq, ExportReport};
use crate::error::AppResult;
use crate::infra::library;

use super::export_util::{collect_assets, execute_copy_plan, read_gltf};

#[tauri::command]
pub async fn export_copy(app: AppHandle, req: ExportCopyReq) -> AppResult<ExportReport> {
    let (assets, mut report) = collect_assets(&app, &req.asset_ids)?;
    let root = library::resolve(&app)?;
    let root = root.to_string_lossy();
    let root = root.as_ref();
    let target = Path::new(&req.target_dir);

    // Track written gltf stems (to guard flatten collisions) and copied textures
    // (to dedup across the batch).
    let mut used_stems: HashSet<String> = HashSet::new();
    let mut copied_textures: HashSet<String> = HashSet::new();

    for asset in &assets {
        if matches!(asset.content, AssetContent::Audio {..}) {
            let stem=resolve_stem(asset,req.preserve_structure,&mut used_stems,&mut report);
            copy_audio(root,target,asset,&stem,req.preserve_structure,&mut report)?;continue;
        }
        if matches!(asset.content, AssetContent::Material { .. }) {
            let stem=resolve_stem(asset,req.preserve_structure,&mut used_stems,&mut report);
            copy_material_named(root, target, asset, &stem, req.preserve_structure, &mut report)?;
            continue;
        }
        let doc = read_gltf(root, asset)?;
        let stem = resolve_stem(asset, req.preserve_structure, &mut used_stems, &mut report);

        let fileset = asset.fileset()?;
        let plan = plan_copy(
            &doc,
            &fileset.gltf,
            &fileset.bin,
            &asset.pack,
            &asset.category,
            &stem,
            req.preserve_structure,
        )?;

        execute_copy_plan(root, target, &plan, &mut copied_textures, &mut report)?;
    }

    Ok(report)
}

/// Choose the output stem, namespacing with the pack on a flatten collision.
pub(super) fn resolve_stem(
    asset: &Asset,
    preserve_structure: bool,
    used: &mut HashSet<String>,
    report: &mut ExportReport,
) -> String {
    // In preserve mode the pack/category dirs disambiguate, so collisions can't
    // happen; only flatten needs the guard.
    let base = crate::domain::animation::export_stem(asset);
    let category = if matches!(asset.content,AssetContent::Material{..}) {"materials"} else {&asset.category};
    let key = |stem:&str| if preserve_structure {format!("{}/{}/{stem}",asset.pack,category)} else {stem.into()};
    if used.insert(key(&base)) {
        return base;
    }
    let prefix = format!("{}_{}", asset.pack, base);
    let mut namespaced = prefix.clone();
    let mut index = 2;
    while !used.insert(key(&namespaced)) { namespaced = format!("{prefix}_{index}"); index += 1; }
    report.warn(format!(
        "renamed '{}' -> '{}' to avoid an export name collision",
        asset.file_name, namespaced
    ));
    namespaced
}

#[cfg(test)]
pub(super) fn material_paths(asset: &Asset, preserve: bool) -> Vec<(String, String)> {
    material_paths_named(asset,&asset.name,preserve)
}
pub(super) fn material_paths_named(asset: &Asset, stem:&str, preserve: bool) -> Vec<(String, String)> {
    let AssetContent::Material { material } = &asset.content else { return Vec::new() };
    let folder = if preserve { format!("{}/materials/{}", asset.pack, stem) }
        else { format!("{}_{}", asset.pack, stem) };
    material.textures().into_iter().map(|src| {
        let name = Path::new(&src).file_name().unwrap_or_default().to_string_lossy();
        let dst = format!("{folder}/{name}");
        (src, dst)
    }).collect()
}

pub(super) fn audio_path(asset:&Asset,stem:&str,preserve:bool)->AppResult<String> {
    let AssetContent::Audio {audio}=&asset.content else {return Err(crate::error::AppError::msg("Expected audio"));};
    // Preserve the logical export layout, never expose opaque storage keys.
    if preserve {Ok(format!("{}/{}/{}.{}",asset.pack,asset.category,stem,Path::new(&audio.file).extension().unwrap_or_default().to_string_lossy()))}
    else {Ok(format!("{stem}.{}",Path::new(&audio.file).extension().unwrap_or_default().to_string_lossy()))}
}
pub(crate) fn copy_audio(root:&str,target:&Path,asset:&Asset,stem:&str,preserve:bool,report:&mut ExportReport)->AppResult<()> {
    let AssetContent::Audio {audio}=&asset.content else {return Err(crate::error::AppError::msg("Expected audio"));};
    let dst=audio_path(asset,stem,preserve)?;
    let source=Path::new(root).join(&audio.file).canonicalize()?;
    let library=Path::new(root).join("library").canonicalize()?;
    if !source.starts_with(library)||Path::new(&dst).components().any(|c|!matches!(c,std::path::Component::Normal(_))) {return Err(crate::error::AppError::msg("Audio file path escapes its library"));}
    crate::infra::fsio::write_bytes(&target.join(&dst),&crate::infra::fsio::read_bytes(&source)?)?;report.write(dst);Ok(())
}

#[cfg(test)]
pub(super) fn copy_material(root: &str, target: &Path, asset: &Asset, preserve: bool, report: &mut ExportReport) -> AppResult<()> {
    copy_material_named(root,target,asset,&asset.name,preserve,report)
}
pub(super) fn copy_material_named(root: &str, target: &Path, asset: &Asset, stem:&str, preserve: bool, report: &mut ExportReport) -> AppResult<()> {
    for (src, dst) in material_paths_named(asset, stem, preserve) {
        let bytes = crate::infra::fsio::read_bytes(&crate::domain::paths::abs_under_root(root, &src))?;
        crate::infra::fsio::write_bytes(&target.join(&dst), &bytes)?;
        report.write(dst);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn material_copy_deduplicates_packed_maps_and_matches_planned_paths() {
        let root = std::env::temp_dir().join(format!("toybox-material-export-{}", std::process::id()));
        let source = root.join("library/p/materials/Steel_2K");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("base.png"), b"base").unwrap();
        std::fs::write(source.join("orm.png"), b"packed").unwrap();
        let asset: Asset = serde_json::from_value(json!({
            "id":"steel", "kind":"material", "name":"Steel_2K", "fileName":"Steel_2K", "relPath":"library/p/materials/Steel_2K", "pack":"p", "category":"metal",
            "thumb":{"state":"missing"}, "user":{"favorite":false,"tags":[]},
            "material":{"maps":[
                {"role":"albedo","file":"library/p/materials/Steel_2K/base.png"},
                {"role":"roughness","file":"library/p/materials/Steel_2K/orm.png","channel":"g"},
                {"role":"metallic","file":"library/p/materials/Steel_2K/orm.png","channel":"b"}
            ]}
        })).unwrap();
        for preserve in [true, false] {
            let target = root.join(if preserve { "structured" } else { "flat" });
            let planned: Vec<_> = material_paths(&asset, preserve).into_iter().map(|(_, dst)| dst).collect();
            assert_eq!(planned.len(), 2);
            let mut report = ExportReport::default();
            copy_material(&root.to_string_lossy(), &target, &asset, preserve, &mut report).unwrap();
            assert_eq!(report.written, planned);
            assert_eq!(std::fs::read(target.join(&planned[1])).unwrap(), b"packed");
        }
        // Category is metadata: two identically named materials in different
        // categories still share an export directory and must not overwrite.
        let mut other=asset.clone();other.id="other".into();other.category="different".into();
        let mut used=HashSet::new();let mut report=ExportReport::default();
        let first=resolve_stem(&asset,true,&mut used,&mut report);
        let second=resolve_stem(&other,true,&mut used,&mut report);
        let first=material_paths_named(&asset,&first,true);
        let second=material_paths_named(&other,&second,true);
        assert!(first.iter().all(|(_,path)|!second.iter().any(|(_,other)|path==other)));
        std::fs::remove_dir_all(root).unwrap();
    }
}
