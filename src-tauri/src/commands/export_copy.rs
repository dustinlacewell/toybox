//! `export_copy`: copy selected assets as self-contained loose glTF filesets
//! into a target directory, rewriting texture/buffer URIs so each export stands
//! alone. Textures are deduped across the batch; flatten-mode glTF basename
//! collisions are guarded by namespacing the stem with the pack.

use std::collections::HashSet;
use std::path::Path;

use tauri::AppHandle;

use crate::config;
use crate::domain::catalog_model::Asset;
use crate::domain::export_copy_plan::{plan_copy, CopyPlan};
use crate::domain::export_model::{ExportCopyReq, ExportReport};
use crate::domain::paths;
use crate::error::AppResult;
use crate::infra::fsio;

use super::export_util::{collect_assets, read_gltf};

#[tauri::command]
pub async fn export_copy(app: AppHandle, req: ExportCopyReq) -> AppResult<ExportReport> {
    let (assets, mut report) = collect_assets(&app, &req.asset_ids)?;
    let root = config::LIBRARY_ROOT;
    let target = Path::new(&req.target_dir);

    // Track written gltf stems (to guard flatten collisions) and copied textures
    // (to dedup across the batch).
    let mut used_stems: HashSet<String> = HashSet::new();
    let mut copied_textures: HashSet<String> = HashSet::new();

    for asset in &assets {
        let doc = read_gltf(root, asset)?;
        let stem = resolve_stem(asset, req.preserve_structure, &mut used_stems, &mut report);

        let plan = plan_copy(
            &doc,
            &asset.fileset.gltf,
            &asset.fileset.bin,
            &asset.pack,
            &asset.category,
            &stem,
            req.preserve_structure,
        )?;

        execute_plan(root, target, &plan, &mut copied_textures, &mut report)?;
    }

    Ok(report)
}

/// Choose the output stem, namespacing with the pack on a flatten collision.
fn resolve_stem(
    asset: &Asset,
    preserve_structure: bool,
    used: &mut HashSet<String>,
    report: &mut ExportReport,
) -> String {
    // In preserve mode the pack/category dirs disambiguate, so collisions can't
    // happen; only flatten needs the guard.
    let base = asset.name.clone();
    if preserve_structure {
        return base;
    }
    if used.insert(base.clone()) {
        return base;
    }
    let namespaced = format!("{}_{}", asset.pack, asset.name);
    report.warn(format!(
        "renamed '{}' -> '{}' to avoid a flattened name collision",
        asset.file_name, namespaced
    ));
    used.insert(namespaced.clone());
    namespaced
}

/// Write the rewritten glTF, copy the bin, and copy any not-yet-copied textures.
fn execute_plan(
    root: &str,
    target: &Path,
    plan: &CopyPlan,
    copied_textures: &mut HashSet<String>,
    report: &mut ExportReport,
) -> AppResult<()> {
    // glTF
    let gltf_text = serde_json::to_string_pretty(&plan.gltf)?;
    let gltf_out = target.join(&plan.gltf_dst);
    fsio::write_text(&gltf_out, &gltf_text)?;
    report.write(plan.gltf_dst.clone());

    // bin
    let bin_bytes = fsio::read_bytes(&paths::abs_under_root(root, &plan.bin_src_rel))?;
    let bin_out = target.join(&plan.bin_dst);
    fsio::write_bytes(&bin_out, &bin_bytes)?;
    report.write(plan.bin_dst.clone());

    // textures (deduped across the batch by target-relative path)
    for tex in &plan.textures {
        let dst_rel = format!("{}/{}", plan.textures_dst_dir, tex.dst_basename);
        if !copied_textures.insert(dst_rel.clone()) {
            continue;
        }
        let src = paths::abs_under_root(root, &tex.src_rel);
        if !fsio::exists(&src) {
            report.skip(format!("missing texture: {}", tex.src_rel));
            continue;
        }
        let bytes = fsio::read_bytes(&src)?;
        fsio::write_bytes(&target.join(&dst_rel), &bytes)?;
        report.write(dst_rel);
    }

    Ok(())
}
