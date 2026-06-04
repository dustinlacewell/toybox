//! `export_placer`: one-way publish of selected assets into a Godot project's
//! asset_placer library. Each asset is baked to a self-contained `.glb` (or
//! copied as a loose `.gltf` fileset) under `<target_dir>/<sub_dir>`, then the
//! project's `asset_library.json` is created or idempotently merged so the
//! exported assets appear in the addon's dock. Capital's own facets — pack,
//! category, favorite, user-tags — become the addon's collections.
//!
//! This is outbound only: Capital never re-seeds its catalog from the file it
//! writes here (see `domain::seed` for the inbound source format).

use std::collections::HashSet;
use std::path::Path;

use tauri::AppHandle;

use crate::domain::catalog_model::Asset;
use crate::domain::export_copy_plan::plan_copy;
use crate::domain::export_model::{ExportPlacerReq, ExportReport, PlacerFormat};
use crate::domain::glb_assemble::assemble;
use crate::domain::placer_library::{self, PlacerAsset};
use crate::domain::paths;
use crate::error::AppResult;
use crate::infra::fsio;

use super::export_glb::gather_images;
use super::export_util::{collect_assets, library_root, read_bin, read_gltf};

#[tauri::command]
pub async fn export_placer(app: AppHandle, req: ExportPlacerReq) -> AppResult<ExportReport> {
    let (assets, mut report) = collect_assets(&app, &req.asset_ids)?;
    let root = library_root();
    let sub_dir = trim_slashes(&req.sub_dir);

    let mut used_stems: HashSet<String> = HashSet::new();
    let mut placer_assets: Vec<PlacerAsset> = Vec::new();

    for asset in &assets {
        let stem = resolve_stem(asset, req.preserve_structure, &mut used_stems, &mut report);
        let written = match req.format {
            PlacerFormat::Glb => {
                write_glb(root, asset, &req, &sub_dir, &stem, &mut report)?
            }
            PlacerFormat::Copy => {
                write_copy(root, asset, &req, &sub_dir, &stem, &mut report)?
            }
        };
        placer_assets.push(to_placer_asset(asset, written));
    }

    merge_library(&req, &sub_dir, &placer_assets)?;
    Ok(report)
}

/// The `res://` id and stored basename of one written asset.
struct Written {
    res_path: String,
    name: String,
}

/// Bake one asset to a self-contained `.glb` under `<target>/<sub_dir>/…`.
fn write_glb(
    root: &str,
    asset: &Asset,
    req: &ExportPlacerReq,
    sub_dir: &str,
    stem: &str,
    report: &mut ExportReport,
) -> AppResult<Written> {
    let doc = read_gltf(root, asset)?;
    let bin = read_bin(root, asset)?;
    let images = gather_images(root, asset, &doc, report)?;
    let glb = assemble(&doc, &bin, images)?;

    let rel = layout_rel(req.preserve_structure, asset, &format!("{stem}.glb"));
    fsio::write_bytes(&under(&req.target_dir, sub_dir, &rel), &glb)?;
    let full = join_rel(sub_dir, &rel);
    report.write(full.clone());
    Ok(Written { res_path: res_uri(&full), name: format!("{stem}.glb") })
}

/// Copy one asset as a loose `.gltf` + `.bin` + textures under `<sub_dir>`.
fn write_copy(
    root: &str,
    asset: &Asset,
    req: &ExportPlacerReq,
    sub_dir: &str,
    stem: &str,
    report: &mut ExportReport,
) -> AppResult<Written> {
    let doc = read_gltf(root, asset)?;
    let plan = plan_copy(
        &doc,
        &asset.fileset.gltf,
        &asset.fileset.bin,
        &asset.pack,
        &asset.category,
        stem,
        req.preserve_structure,
    )?;
    let base = under_dir(&req.target_dir, sub_dir);

    // glTF
    let gltf_text = serde_json::to_string_pretty(&plan.gltf)?;
    fsio::write_text(&base.join(&plan.gltf_dst), &gltf_text)?;
    report.write(join_rel(sub_dir, &plan.gltf_dst));

    // bin
    let bin_bytes = fsio::read_bytes(&paths::abs_under_root(root, &plan.bin_src_rel))?;
    fsio::write_bytes(&base.join(&plan.bin_dst), &bin_bytes)?;
    report.write(join_rel(sub_dir, &plan.bin_dst));

    // textures (deduped within this asset's plan; cross-asset dedup is by
    // identical dst path, which write_bytes overwrites harmlessly)
    for tex in &plan.textures {
        let dst_rel = format!("{}/{}", plan.textures_dst_dir, tex.dst_basename);
        let src = paths::abs_under_root(root, &tex.src_rel);
        if !fsio::exists(&src) {
            report.skip(format!("missing texture: {}", tex.src_rel));
            continue;
        }
        let bytes = fsio::read_bytes(&src)?;
        fsio::write_bytes(&base.join(&dst_rel), &bytes)?;
        report.write(join_rel(sub_dir, &dst_rel));
    }

    let full = join_rel(sub_dir, &plan.gltf_dst);
    Ok(Written { res_path: res_uri(&full), name: format!("{stem}.gltf") })
}

/// Read-or-empty the target `asset_library.json`, merge in the published
/// assets and the recursive root folder, and write it back.
fn merge_library(req: &ExportPlacerReq, sub_dir: &str, assets: &[PlacerAsset]) -> AppResult<()> {
    let lib_path = Path::new(&req.library_json_path);
    let existing = if fsio::exists(lib_path) {
        let text = fsio::read_text(lib_path)?;
        serde_json::from_str(&text).unwrap_or_else(|_| placer_library::empty_library())
    } else {
        placer_library::empty_library()
    };
    let mut merged = placer_library::merge(&existing, assets);
    placer_library::ensure_folder(&mut merged, &res_uri(sub_dir));
    fsio::write_text(lib_path, &serde_json::to_string_pretty(&merged)?)
}

fn to_placer_asset(asset: &Asset, written: Written) -> PlacerAsset {
    PlacerAsset {
        pack: asset.pack.clone(),
        category: asset.category.clone(),
        favorite: asset.user.favorite,
        tags: asset.user.tags.clone(),
        res_path: written.res_path,
        name: written.name,
    }
}

/// Flatten-only collision guard: namespace a duplicate stem with the pack.
/// (Preserve mode can't collide — the pack/category dirs disambiguate.)
fn resolve_stem(
    asset: &Asset,
    preserve_structure: bool,
    used: &mut HashSet<String>,
    report: &mut ExportReport,
) -> String {
    if preserve_structure || used.insert(asset.name.clone()) {
        return asset.name.clone();
    }
    let namespaced = format!("{}_{}", asset.pack, asset.name);
    report.warn(format!(
        "renamed '{}' -> '{}' to avoid a flattened name collision",
        asset.name, namespaced
    ));
    used.insert(namespaced.clone());
    namespaced
}

/// Target-relative path of an asset file given the layout.
fn layout_rel(preserve_structure: bool, asset: &Asset, file: &str) -> String {
    if preserve_structure {
        format!("{}/{}/{}", asset.pack, asset.category, file)
    } else {
        file.to_string()
    }
}

fn under(target_dir: &str, sub_dir: &str, rel: &str) -> std::path::PathBuf {
    under_dir(target_dir, sub_dir).join(rel)
}

fn under_dir(target_dir: &str, sub_dir: &str) -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(target_dir);
    for seg in sub_dir.split('/').filter(|s| !s.is_empty()) {
        p.push(seg);
    }
    p
}

/// Join a project-relative sub_dir and an asset-relative path with `/`.
fn join_rel(sub_dir: &str, rel: &str) -> String {
    let rel = rel.replace('\\', "/");
    if sub_dir.is_empty() {
        rel
    } else {
        format!("{sub_dir}/{rel}")
    }
}

/// Form a `res://assets/...`-style id from a project-relative path.
fn res_uri(project_rel: &str) -> String {
    format!("res://{}", project_rel.trim_start_matches('/'))
}

fn trim_slashes(s: &str) -> String {
    s.trim_matches('/').replace('\\', "/")
}
