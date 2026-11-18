//! Materialize independent animation files before atomically registering them.
//! Original model files remain untouched; model exports omit embedded clips.
use crate::{
    domain::{
        animation, animation_extract,
        catalog_model::{Asset, AssetFileset},
        paths, seed,
    },
    error::{AppError, AppResult},
};
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path};

pub fn prepare(root: &str, mut manifest: Value) -> AppResult<Value> {
    let entries = manifest["assets"]
        .as_array_mut()
        .ok_or_else(|| AppError::msg("Missing source assets"))?;
    let mut generated = Vec::new();
    for entry in entries.iter_mut() {
        prepare_entry(root, entry, &mut generated)?;
    }
    // Replacing an animation-only source removes its dependency on that source.
    entries.retain(|entry| entry.get("_toyboxReplaceSource").is_none());
    let mut ids: HashSet<String> = entries
        .iter()
        .filter_map(|entry| entry["id"].as_str().map(String::from))
        .collect();
    for entry in generated {
        let id = entry["id"].as_str().unwrap().to_string();
        if let Some(prior) = entries.iter_mut().find(|prior| prior["id"] == entry["id"]) {
            *prior = entry;
        } else if ids.insert(id) {
            entries.push(entry);
        }
    }
    Ok(manifest)
}

/// What [`prepare_entries`] made of a list of entries.
pub struct Prepared {
    /// The entries to store: marked as extracted where they were, without the
    /// animation-only sources that clips replace.
    pub entries: Vec<Value>,
    /// The independent clip entries cut from them.
    pub generated: Vec<Value>,
    /// Ids of the animation-only sources that were dropped (their clips replace them).
    pub replaced: Vec<String>,
}

/// [`prepare`] for the entries of one commit alone: the same extraction, without
/// a manifest around it. The caller places the clip entries (a clip replaces an
/// item of the same id, otherwise it is new).
pub fn prepare_entries(root: &str, mut entries: Vec<Value>) -> AppResult<Prepared> {
    let mut generated = Vec::new();
    for entry in entries.iter_mut() {
        prepare_entry(root, entry, &mut generated)?;
    }
    let replaced = entries.iter().filter(|entry| entry.get("_toyboxReplaceSource").is_some()).filter_map(|entry| entry["id"].as_str().map(String::from)).collect();
    entries.retain(|entry| entry.get("_toyboxReplaceSource").is_none());
    Ok(Prepared { entries, generated, replaced })
}

fn prepare_entry(root: &str, entry: &mut Value, generated: &mut Vec<Value>) -> AppResult<()> {
    if let Some(variants) = entry.get_mut("variants").and_then(Value::as_array_mut) {
        for variant in variants {
            prepare_entry(root, variant, generated)?;
        }
    }
    let kind = entry["kind"].as_str().unwrap_or("model");
    if !matches!(kind, "model" | "animation")
        || entry["standalone"].as_bool() == Some(true)
        || entry["animationsExtracted"].as_bool() == Some(true)
    {
        return Ok(());
    }
    let animation_only = kind == "animation";
    let parsed = seed::parse_entry(entry)?;
    if parsed.pack.is_empty()
        || parsed.pack == "."
        || parsed.pack == ".."
        || parsed.pack.contains(['/', '\\', ':'])
    {
        return Err(AppError::msg(
            "Animation pack must be a single directory name",
        ));
    }
    let rel = parsed.rel_path.clone();
    let source_path = paths::abs_under_root(root, &rel);
    let canonical_root = Path::new(root).canonicalize()?;
    if !source_path.canonicalize()?.starts_with(&canonical_root) {
        return Err(AppError::msg("Animation source is outside its library"));
    }
    let doc: Value = serde_json::from_slice(&std::fs::read(&source_path)?)?;
    let fileset = AssetFileset {
        gltf: rel.clone(),
        bin: rel.trim_end_matches(".gltf").to_string() + ".bin",
        textures: vec![],
    };
    // Seed's shared identity is sufficient to derive the existing clip IDs.
    let asset: Asset = serde_json::from_value(
        json!({"id":parsed.id,"name":parsed.name,"fileName":parsed.file_name,"relPath":rel,"pack":parsed.pack,"category":parsed.category,
        "kind":"model","fileset":fileset,"thumb":{"state":"missing"},"user":{"favorite":false,"tags":[]}}),
    )?;
    let clips = animation::clips(&doc, &asset, &fileset);
    if clips.is_empty() {
        if animation_only {
            return Err(AppError::msg(format!(
                "{} contains no usable clips",
                asset.file_name
            )));
        }
        return Ok(());
    }
    let mut buffers = Vec::new();
    for buffer in doc["buffers"]
        .as_array()
        .ok_or_else(|| AppError::msg("Missing animation buffers"))?
    {
        let uri = buffer["uri"]
            .as_str()
            .ok_or_else(|| AppError::msg("Animation buffer needs an external URI"))?;
        if let Some((_, bytes)) = crate::domain::gltf_uri::decode_data_uri(uri) {
            buffers.push(bytes);
            continue;
        }
        if crate::domain::gltf_uri::external_file(uri).is_none() {
            return Err(AppError::msg("Animation buffer needs an external URI"));
        }
        let path = source_path.parent().unwrap().join(uri).canonicalize()?;
        if !path.starts_with(&canonical_root) {
            return Err(AppError::msg("Animation buffer is outside its library"));
        }
        buffers.push(std::fs::read(path)?);
    }
    for clip_asset in clips {
        let crate::domain::catalog_model::AssetContent::Animation { clip, .. } =
            &clip_asset.content
        else {
            unreachable!()
        };
        let stem = format!(
            "toybox_{:016x}_{}",
            crate::domain::rig::hash(fileset.gltf.as_bytes()),
            clip.index
        );
        let folder = format!("library/animations/{:016x}",crate::domain::rig::hash(clip_asset.id.as_bytes()));
        let metadata = json!({"id":clip_asset.id,"clip":clip});
        let extracted = animation_extract::extract(
            &doc,
            &buffers,
            clip.index,
            &format!("{stem}.bin"),
            metadata,
        )?;
        let gltf_path = paths::abs_under_root(root, &format!("{folder}/{stem}.gltf"));
        let bin_path = paths::abs_under_root(root, &format!("{folder}/{stem}.bin"));
        let destination = gltf_path.parent().unwrap();
        let ancestor = destination
            .ancestors()
            .find(|path| path.exists())
            .ok_or_else(|| AppError::msg("Animation destination has no parent"))?;
        if !ancestor.canonicalize()?.starts_with(&canonical_root) {
            return Err(AppError::msg(
                "Animation destination is outside its library",
            ));
        }
        if gltf_path.exists() {
            let previous: Value = serde_json::from_slice(&std::fs::read(&gltf_path)?)?;
            if previous["extras"]["toyboxAnimation"]["id"] != clip_asset.id {
                return Err(AppError::msg(
                    "Generated animation path is already occupied",
                ));
            }
        }
        write_if_changed(&bin_path, &extracted.bin)?;
        write_if_changed(&gltf_path, &serde_json::to_vec_pretty(&extracted.doc)?)?;
        let mut generated_entry = json!({"kind":"animation","standalone":true,"id":clip_asset.id,"pack":asset.pack,"category":"animations","file":format!("{stem}.gltf"),"storagePath":format!("{folder}/{stem}.gltf"),"provenance":entry.get("provenance").cloned().unwrap_or(Value::Null),"format":entry.get("format").cloned().unwrap_or(Value::Null),"origin":clip_origin(entry)});
        // An animation file imported with tags describes its clips. A model's own
        // tags describe the model, not the clips cut from it, so those stay behind.
        if animation_only {
            let tags = crate::domain::tags::from_entry(entry);
            if !tags.is_empty() { generated_entry["tags"] = json!(tags); }
        }
        generated.push(generated_entry);
    }
    if animation_only {
        entry["_toyboxReplaceSource"] = json!(true);
    } else {
        entry["animationsExtracted"] = json!(true);
    }
    Ok(())
}

/// What an extracted clip records of where its model came from: the importing
/// plugin and its own record, but not the model's kept source files. Those stay
/// with the model, once, instead of being copied into every clip.
fn clip_origin(entry: &Value) -> Value {
    let Some(origin) = entry.get("origin").filter(|origin| origin.get("plugin").is_some_and(Value::is_string)) else {
        return Value::Null;
    };
    let mut kept = origin.clone();
    if let Some(object) = kept.as_object_mut() {
        object.remove("files");
    }
    kept
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if path.exists() && std::fs::read(path)? == bytes {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = path.with_extension(format!("{}-{stamp}.tmp", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(temp, path)?;
    Ok(())
}

/// Write the manifest `prepare` produced, if it differs from `previous` (the
/// manifest as read, at `revision`). Refused if the manifest moved on meanwhile;
/// the document it replaces is kept beside the database as a safety copy.
pub fn commit(root: &str, previous: &Value, revision: i64, manifest: &Value) -> AppResult<()> {
    if previous == manifest {
        return Ok(());
    }
    let root = std::path::Path::new(root);
    if crate::infra::store::manifest_revision(root)? != revision {
        return Err(AppError::msg(
            "Library manifest changed during animation import; retry",
        ));
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::fs::write(
        root.join("_library_config").join(format!("catalog-before-animation-extraction-{timestamp}.json")),
        serde_json::to_vec_pretty(previous)?,
    )?;
    crate::infra::store::write_manifest_checked(root, manifest, revision).map_err(|error| {
        if error.to_string().contains("changed while it was being edited") {
            AppError::msg("Library manifest changed during animation import; retry")
        } else {
            error
        }
    })
}

#[cfg(test)]
mod origin_tests {
    use super::*;
    #[test]
    fn an_extracted_clip_records_the_plugin_and_meta_but_not_the_kept_files() {
        let entry = json!({"origin":{"plugin":"x","files":["Hero.fbx"],"meta":{"format":"fbx"}}});
        assert_eq!(clip_origin(&entry), json!({"plugin":"x","meta":{"format":"fbx"}}));
        assert_eq!(clip_origin(&json!({"id":"a"})), Value::Null);
        assert_eq!(clip_origin(&json!({"origin":{"files":["a"]}})), Value::Null);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_creates_independent_clips_and_rescan_survives_source_removal() {
        let root = std::env::temp_dir().join(format!(
            "toybox-independent-animation-{}",
            std::process::id()
        ));
        let folder = root.join("library/p/characters");
        std::fs::create_dir_all(&folder).unwrap();
        let times: Vec<u8> = [0.0f32, 1.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let values: Vec<u8> = [0.0f32, 0.0, 0.0, 2.0, 0.0, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let bytes = [times, values].concat();
        let doc = json!({"asset":{"version":"2.0"},"nodes":[{"name":"Root","mesh":0}],"meshes":[{"primitives":[]}],"buffers":[{"uri":"Hero.bin","byteLength":32}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":8},{"buffer":0,"byteOffset":8,"byteLength":24}],"accessors":[{"bufferView":0,"componentType":5126,"type":"SCALAR","count":2,"min":[0],"max":[1]},{"bufferView":1,"componentType":5126,"type":"VEC3","count":2}],"animations":[{"name":"Empty","channels":[]},{"name":"Walk","samplers":[{"input":0,"output":1}],"channels":[{"sampler":0,"target":{"node":0,"path":"translation"}}]}]});
        std::fs::write(folder.join("Hero.gltf"), serde_json::to_vec(&doc).unwrap()).unwrap();
        std::fs::write(folder.join("Hero.bin"), &bytes).unwrap();
        let manifest =
            json!({"assets":[{"id":"hero","pack":"p","category":"characters","file":"Hero.gltf","provenance":{"source":"Locomotion","publisher":"Studio","license":"CC0","filesystemPath":"D:/downloads/Hero.fbx"}}]});
        let prepared = prepare(&root.to_string_lossy(), manifest).unwrap();
        assert_eq!(prepared["assets"].as_array().unwrap().len(), 2);
        assert_eq!(prepared["assets"][0]["animationsExtracted"], true);
        assert_eq!(prepared["assets"][1]["provenance"],prepared["assets"][0]["provenance"]);
        assert_eq!(
            prepare(&root.to_string_lossy(), prepared.clone()).unwrap(),
            prepared,
            "rescan is idempotent"
        );
        let catalog = crate::ops::catalog_build::build_catalog(
            &root.to_string_lossy(),
            seed::parse_seed_entries(&prepared).unwrap(),
        )
        .unwrap();
        assert_eq!(catalog.assets.len(), 2);
        let animation = catalog
            .assets
            .iter()
            .find(|asset| {
                matches!(
                    asset.content,
                    crate::domain::catalog_model::AssetContent::Animation { .. }
                )
            })
            .unwrap();
        let model = catalog
            .assets
            .iter()
            .find(|asset| asset.id == "hero")
            .unwrap();
        assert!(crate::domain::rig::match_animation(animation, model, &catalog.rigs).is_some());
        let clip_doc =
            crate::ops::export_util::read_gltf(&root.to_string_lossy(), animation).unwrap();
        assert!(clip_doc.get("meshes").is_none());
        assert_eq!(clip_doc["animations"][0]["name"], "Walk");
        assert_eq!(
            crate::ops::export_util::read_bin(&root.to_string_lossy(), animation).unwrap(),
            bytes
        );
        let mut changed = doc.clone();
        changed["nodes"][0]["translation"] = json!([0, 9, 0]);
        std::fs::write(
            folder.join("Hero.gltf"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert!(
            crate::ops::animation_preview::read_preview(
                &root.to_string_lossy(),
                &catalog,
                &animation.id,
                &model.id
            )
            .is_err(),
            "cached compatibility cannot bypass changed rest transforms"
        );
        let before = animation.id.clone();
        std::fs::remove_file(folder.join("Hero.gltf")).unwrap();
        std::fs::remove_file(folder.join("Hero.bin")).unwrap();
        let mut removed = prepared.clone();
        removed["assets"]
            .as_array_mut()
            .unwrap()
            .retain(|entry| entry["kind"] == "animation");
        let independent = prepare(&root.to_string_lossy(), removed).unwrap();
        let catalog = crate::ops::catalog_build::build_catalog(
            &root.to_string_lossy(),
            seed::parse_seed_entries(&independent).unwrap(),
        )
        .unwrap();
        assert_eq!(catalog.assets.len(), 1);
        assert_eq!(catalog.assets[0].id, before);
        assert_eq!(catalog.assets[0].user.provenance.as_ref().unwrap().filesystem_path.as_deref(),Some("D:/downloads/Hero.fbx"));
        assert!(catalog.assets[0].user.tags.contains(&"publisher: Studio".to_string()));
        assert!(crate::ops::export_util::read_gltf(
            &root.to_string_lossy(),
            &catalog.assets[0]
        )
        .is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
}
