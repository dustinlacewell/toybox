//! `export_glb`: pack each selected asset into a single self-contained binary
//! glTF (`.glb`) via lossless direct assembly — geometry bytes are preserved,
//! textures are embedded (the lone TGA transcoded to PNG). Per-asset filenames
//! follow the same collision guard as the copy export.

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;
use tauri::AppHandle;

use crate::domain::catalog_model::Asset;
use crate::domain::export_model::{ExportGlbReq, ExportReport};
use crate::domain::glb_assemble::{assemble, EmbedImage};
use crate::domain::image_embed::prepare_for_glb;
use crate::domain::{gltf_parse, paths};
use crate::error::AppResult;
use crate::infra::fsio;

use super::export_util::{collect_assets, library_root, read_bin, read_gltf};

#[tauri::command]
pub async fn export_glb(app: AppHandle, req: ExportGlbReq) -> AppResult<ExportReport> {
    let (assets, mut report) = collect_assets(&app, &req.asset_ids)?;
    let root = library_root();
    let target = Path::new(&req.target_dir);
    let mut used_stems: HashSet<String> = HashSet::new();

    for asset in &assets {
        let doc = read_gltf(root, asset)?;
        let bin = read_bin(root, asset)?;
        let images = gather_images(root, asset, &doc, &mut report)?;

        let glb = assemble(&doc, &bin, images)?;

        let stem = resolve_stem(asset, req.preserve_structure, &mut used_stems, &mut report);
        let dst_rel = if req.preserve_structure {
            format!("{}/{}/{}.glb", asset.pack, asset.category, stem)
        } else {
            format!("{stem}.glb")
        };
        fsio::write_bytes(&target.join(&dst_rel), &glb)?;
        report.write(dst_rel);
    }

    Ok(report)
}

/// Read and GLB-prepare every image referenced by the glTF, keyed to its index.
pub(crate) fn gather_images(
    root: &str,
    asset: &Asset,
    doc: &Value,
    report: &mut ExportReport,
) -> AppResult<Vec<EmbedImage>> {
    let gltf_dir = parent_rel(&asset.fileset.gltf);
    let mut out = Vec::new();
    for img in gltf_parse::image_uris(doc) {
        let tex_rel = paths::resolve_uri_rel(&gltf_dir, &img.uri);
        let abs = paths::abs_under_root(root, &tex_rel);
        if !fsio::exists(&abs) {
            report.skip(format!("missing texture: {tex_rel}"));
            continue;
        }
        let bytes = fsio::read_bytes(&abs)?;
        let file_name = basename(&tex_rel);
        let prepared = prepare_for_glb(file_name, bytes)?;
        if file_name.to_lowercase().ends_with(".tga") {
            report.warn(format!("transcoded {file_name} (TGA) -> PNG for GLB"));
        }
        out.push(EmbedImage {
            image_index: img.index,
            mime: prepared.mime,
            bytes: prepared.bytes,
        });
    }
    Ok(out)
}

/// Same collision guard as the copy export (flatten-only namespacing).
fn resolve_stem(
    asset: &Asset,
    preserve_structure: bool,
    used: &mut HashSet<String>,
    report: &mut ExportReport,
) -> String {
    if preserve_structure {
        return asset.name.clone();
    }
    if used.insert(asset.name.clone()) {
        return asset.name.clone();
    }
    let namespaced = format!("{}_{}", asset.pack, asset.name);
    report.warn(format!(
        "renamed '{}.glb' -> '{}.glb' to avoid a flattened name collision",
        asset.name, namespaced
    ));
    used.insert(namespaced.clone());
    namespaced
}

fn parent_rel(rel: &str) -> String {
    let norm = rel.replace('\\', "/");
    match norm.rfind('/') {
        Some(i) => norm[..i].to_string(),
        None => String::new(),
    }
}

fn basename(rel: &str) -> &str {
    match rel.rfind(['/', '\\']) {
        Some(i) => &rel[i + 1..],
        None => rel,
    }
}
