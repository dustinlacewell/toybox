//! `export_copy`: copy selected assets as self-contained loose glTF filesets
//! into a target directory, rewriting texture/buffer URIs so each export stands
//! alone. Textures are deduped across the batch; flatten-mode glTF basename
//! collisions are guarded by namespacing the stem with the pack.

use std::collections::HashSet;
use std::path::Path;

use tauri::AppHandle;

use crate::config;
use crate::domain::catalog_model::Asset;
use crate::domain::export_copy_plan::plan_copy;
use crate::domain::export_model::{ExportCopyReq, ExportReport};
use crate::error::AppResult;

use super::export_util::{collect_assets, execute_copy_plan, read_gltf};

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

        execute_copy_plan(root, target, &plan, &mut copied_textures, &mut report)?;
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
