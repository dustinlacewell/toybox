//! Shared helpers for the export commands: resolve catalog assets by id and read
//! their parsed glTF documents.

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

use crate::domain::catalog_model::Asset;
use crate::domain::export_copy_plan::CopyPlan;
use crate::domain::export_model::ExportReport;
use crate::domain::{gltf_parse, paths};
use crate::error::{AppError, AppResult};
use crate::infra::fsio;
use crate::infra::registry::LibraryHandle;

use crate::infra::store::Store;

/// Resolve the requested asset ids against the catalog. Missing ids are recorded
/// in the report's `skipped` list rather than failing the whole export.
pub fn collect_assets(
    handle: &LibraryHandle,
    asset_ids: &[String],
) -> AppResult<(Vec<Asset>, ExportReport)> {
    super::catalog::open_library(handle)?;
    let store = Store::for_handle(handle);
    if !store.catalog_exists() {
        return Err(AppError::msg("no catalog; scan first"));
    }
    let mut report = ExportReport::default();
    // Only the rows of the asked ids are read, whether they are items or variants.
    let mut found: std::collections::HashMap<String, Asset> = store.load_assets(asset_ids)?.into_iter().map(|asset| (asset.id.clone(), asset)).collect();
    let mut assets = Vec::new();
    for id in asset_ids {
        match found.get(id) {
            Some(a) => assets.push(a.clone()),
            None => report.skip(format!("asset not found: {id}")),
        }
    }
    found.clear();
    // Everything the callers do next reads these items' files by path; a remote library fetches what it lacks first.
    handle.ensure_assets(&assets)?;
    Ok((assets, report))
}

/// Read and parse an asset's glTF document.
pub fn read_gltf(root: &str, asset: &Asset) -> AppResult<Value> {
    let abs = paths::abs_under_root(root, &asset.fileset()?.gltf);
    let text = fsio::read_text(&abs)?;
    let mut doc = gltf_parse::parse(&text)?;
    if crate::domain::gltf_vertex_colors::may_need_repair(&doc) {
        crate::domain::gltf_vertex_colors::repair(&mut doc, &[read_bin(root, asset)?]);
    }
    match &asset.content {
        crate::domain::catalog_model::AssetContent::Animation { clip, .. } => crate::domain::animation::select_clip(doc, clip.index),
        _ => { doc.as_object_mut().unwrap().remove("animations"); Ok(doc) },
    }
}

/// Read an asset's single buffer (`.bin`) bytes.
pub fn read_bin(root: &str, asset: &Asset) -> AppResult<Vec<u8>> {
    fsio::read_bytes(&paths::abs_under_root(root, &asset.fileset()?.bin))
}

/// Write a planned loose-copy fileset (rewritten glTF + bin + textures) under
/// `target`. Textures already present in `copied_textures` (by target-relative
/// path) are skipped, so a caller looping many assets dedups shared atlases
/// across the batch; a single-asset caller can pass a fresh set. Written paths
/// are recorded in `report`.
pub fn execute_copy_plan(
    root: &str,
    target: &Path,
    plan: &CopyPlan,
    copied_textures: &mut HashSet<String>,
    report: &mut ExportReport,
) -> AppResult<()> {
    // glTF
    let gltf_text = serde_json::to_string_pretty(&plan.gltf)?;
    fsio::write_text(&target.join(&plan.gltf_dst), &gltf_text)?;
    report.write(plan.gltf_dst.clone());

    // bin
    let bin_bytes = fsio::read_bytes(&paths::abs_under_root(root, &plan.bin_src_rel))?;
    fsio::write_bytes(&target.join(&plan.bin_dst), &bin_bytes)?;
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
