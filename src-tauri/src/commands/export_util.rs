//! Shared helpers for the export commands: resolve catalog assets by id and read
//! their parsed glTF documents.

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;
use tauri::AppHandle;

use crate::config;
use crate::domain::catalog_model::Asset;
use crate::domain::export_copy_plan::CopyPlan;
use crate::domain::export_model::ExportReport;
use crate::domain::{gltf_parse, paths};
use crate::error::{AppError, AppResult};
use crate::infra::fsio;

use super::catalog::load_catalog_inner;

/// Resolve the requested asset ids against the catalog. Missing ids are recorded
/// in the report's `skipped` list rather than failing the whole export.
pub fn collect_assets(
    app: &AppHandle,
    asset_ids: &[String],
) -> AppResult<(Vec<Asset>, ExportReport)> {
    let catalog =
        load_catalog_inner(app)?.ok_or_else(|| AppError::msg("no catalog; scan first"))?;
    let mut report = ExportReport::default();
    let mut assets = Vec::new();
    for id in asset_ids {
        match catalog.assets.iter().find(|a| &a.id == id) {
            Some(a) => assets.push(a.clone()),
            None => report.skip(format!("asset not found: {id}")),
        }
    }
    Ok((assets, report))
}

/// Read and parse an asset's glTF document.
pub fn read_gltf(root: &str, asset: &Asset) -> AppResult<Value> {
    let abs = paths::abs_under_root(root, &asset.fileset.gltf);
    let text = fsio::read_text(&abs)?;
    gltf_parse::parse(&text)
}

/// Read an asset's single buffer (`.bin`) bytes.
pub fn read_bin(root: &str, asset: &Asset) -> AppResult<Vec<u8>> {
    fsio::read_bytes(&paths::abs_under_root(root, &asset.fileset.bin))
}

/// Library root accessor (keeps commands from each reaching into config).
pub fn library_root() -> &'static str {
    config::LIBRARY_ROOT
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
