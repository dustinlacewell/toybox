//! Backed-up repair of the narrowly identified legacy FBX vertex-colour mask.
use std::{collections::BTreeSet, path::Path};
use crate::{domain::{seed::{self, SeedContent, SeedEntry}, gltf_vertex_colors}, error::AppResult};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairReport { pub scanned: usize, pub changed: Vec<String>, pub backup: Option<String> }

pub fn repair_colors(root: &Path, apply: bool) -> Result<RepairReport, String> {
    repair_inner(root, apply).map_err(|error| error.to_string())
}

fn collect(entry: &SeedEntry, paths: &mut BTreeSet<(String, String)>) {
    match &entry.content {
        SeedContent::Model { gltf_rel, bin_rel } | SeedContent::AnimationSource { gltf_rel, bin_rel } => { paths.insert((gltf_rel.clone(), bin_rel.clone())); },
        _ => {},
    }
    for variant in &entry.variants { collect(variant, paths); }
}

fn repair_inner(root: &Path, apply: bool) -> AppResult<RepairReport> {
    // A library nobody opened since the databases arrived still has its JSON manifest.
    let doc = match crate::infra::store::read_manifest(root) {
        Ok(doc) => doc,
        Err(error) => match std::fs::read(root.join(crate::config::SEED_REL)) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(_) => return Err(error),
        },
    };
    let mut files = BTreeSet::new();
    for entry in seed::parse_seed_entries(&doc)? { collect(&entry, &mut files); }
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let backup = root.join("_library_config").join(format!("vertex-color-backup-{timestamp}"));
    let mut report = RepairReport { scanned: files.len(), changed: vec![], backup: None };
    let canonical_root = root.canonicalize()?;
    for (gltf_rel, bin_rel) in files {
        let path = root.join(&gltf_rel).canonicalize()?;
        let bin = root.join(&bin_rel).canonicalize()?;
        if !path.starts_with(&canonical_root) || !bin.starts_with(&canonical_root) { return Err(crate::error::AppError::msg("Scene files must remain inside the library")); }
        let original = std::fs::read(&path)?;
        let mut scene: serde_json::Value = serde_json::from_slice(&original)?;
        if !gltf_vertex_colors::may_need_repair(&scene) { continue; }
        if gltf_vertex_colors::repair(&mut scene, &[std::fs::read(bin)?]).is_empty() { continue; }
        report.changed.push(gltf_rel.clone());
        if apply {
            let relative = path.strip_prefix(&canonical_root).unwrap();
            let destination = backup.join(relative);
            std::fs::create_dir_all(destination.parent().unwrap())?;
            std::fs::write(destination, &original)?;
            if std::fs::read(&path)? != original { return Err(crate::error::AppError::msg("Scene changed during repair")); }
            let temp = path.with_extension(format!("repair-{timestamp}.tmp"));
            std::fs::write(&temp, serde_json::to_vec_pretty(&scene)?)?;
            std::fs::rename(temp, path)?;
            report.backup = Some(backup.to_string_lossy().to_string());
        }
    }
    if apply && !report.changed.is_empty() { std::fs::write(backup.join("report.json"), serde_json::to_vec_pretty(&report)?)?; }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn repair_is_backed_up_and_idempotent_without_changing_binary_or_uvs() {
        let root = std::env::temp_dir().join(format!("toybox-color-repair-test-{}", std::process::id()));
        let folder = root.join("library/p/characters");
        std::fs::create_dir_all(&folder).unwrap(); std::fs::create_dir_all(root.join("_library_config")).unwrap();
        let manifest = json!({"assets":[{"id":"hero","pack":"p","category":"characters","file":"Hero.gltf"}]});
        crate::infra::store::write_manifest(&root, &manifest).unwrap();
        let scene = json!({"materials":[{"extras":{"fromFBX":{"isTruePBR":false}},"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}],"meshes":[{"primitives":[{"material":0,"attributes":{"POSITION":1,"TEXCOORD_0":2,"COLOR_0":0}}]}],"bufferViews":[{"buffer":0,"byteLength":16}],"accessors":[{"type":"VEC4","componentType":5126,"count":1,"bufferView":0}]});
        let original = serde_json::to_vec(&scene).unwrap();
        std::fs::write(folder.join("Hero.gltf"), &original).unwrap(); std::fs::write(folder.join("Hero.bin"), [0; 16]).unwrap();
        let dry = repair_colors(&root, false).unwrap(); assert_eq!(dry.changed.len(), 1); assert!(dry.backup.is_none());
        assert_eq!(std::fs::read(folder.join("Hero.gltf")).unwrap(), original);
        let applied = repair_colors(&root, true).unwrap();
        assert_eq!(std::fs::read(Path::new(applied.backup.as_ref().unwrap()).join("library/p/characters/Hero.gltf")).unwrap(), original);
        let fixed: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.join("Hero.gltf")).unwrap()).unwrap();
        assert_eq!(fixed["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"], 2);
        assert_eq!(std::fs::read(folder.join("Hero.bin")).unwrap(), [0; 16]);
        assert!(repair_colors(&root, true).unwrap().changed.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
