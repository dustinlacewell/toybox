//! Clip-level catalog records and lossless single-clip export selection.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::domain::catalog_model::{Asset, AssetContent, AssetFileset, ThumbMeta, UserMeta};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackTarget { pub node: usize, pub path: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipMeta {
    #[serde(default)]
    pub tracks: Vec<TrackTarget>,
    #[serde(default)]
    pub standalone: bool,
    /// Index in the original glTF animations array, including empty entries.
    pub index: usize,
    pub name: String,
    pub duration: f64,
    pub source_id: String,
    pub source_name: String,
    pub target_nodes: Vec<String>,
    pub rig: String,
}

pub fn clips(doc: &Value, source: &Asset, fileset: &AssetFileset) -> Vec<Asset> {
    let Some(animations) = doc["animations"].as_array() else { return vec![] };
    let identity_path = doc["extras"]["toyboxSourcePath"].as_str().or_else(||doc["extensions"]["TOYBOX_library"]["sourcePath"].as_str()).unwrap_or(&fileset.gltf);
    let key = identity_path.bytes().fold(0xcbf29ce484222325u64, |hash, byte| (hash ^ byte as u64).wrapping_mul(0x100000001b3));
    animations.iter().enumerate().filter_map(|(index, animation)| {
        let channels = animation["channels"].as_array()?;
        if channels.is_empty() { return None; }
        let name = animation["name"].as_str().filter(|name| !name.is_empty()).map(String::from).unwrap_or_else(|| format!("Clip {}", index + 1));
        let mut targets = Vec::new();
        let mut target_indices = Vec::new();
        let mut tracks = Vec::new();
        let mut duration = 0.0f64;
        for channel in channels {
            let node_index = channel["target"]["node"].as_u64()? as usize;
            let node = doc["nodes"].get(node_index)?;
            target_indices.push(node_index);
            tracks.push(TrackTarget { node: node_index, path: channel["target"]["path"].as_str()?.to_string() });
            let target_name = node["name"].as_str().map(String::from).unwrap_or_else(|| format!("node_{node_index}"));
            if !targets.contains(&target_name) { targets.push(target_name); }
            let sampler = animation["samplers"].get(channel["sampler"].as_u64()? as usize)?;
            let accessor = doc["accessors"].get(sampler["input"].as_u64()? as usize)?;
            let end = accessor["max"][0].as_f64()?;
            if !end.is_finite() || end < 0.0 { return None; }
            duration = duration.max(end);
        }
        let rig = doc["skins"].as_array().and_then(|skins| {
            let mesh_skin = target_indices.iter().find_map(|index| doc["nodes"][*index]["skin"].as_u64()).and_then(|index| skins.get(index as usize));
            mesh_skin.or_else(|| skins.iter().find(|skin| skin["skeleton"].as_u64().is_some_and(|index| target_indices.contains(&(index as usize)))
                || skin["joints"].as_array().is_some_and(|joints| joints.iter().any(|joint| joint.as_u64().is_some_and(|index| target_indices.contains(&(index as usize)))))))
        }).and_then(|skin| {
            skin["name"].as_str().map(String::from).or_else(|| skin["skeleton"].as_u64().and_then(|index| doc["nodes"][index as usize]["name"].as_str()).map(String::from))
        }).unwrap_or_else(|| "Node animation".into());
        let saved = &doc["extras"]["toyboxAnimation"];
        let standalone = saved["id"].as_str().is_some();
        let source_id = saved["clip"]["sourceId"].as_str().unwrap_or(&source.id).to_string();
        let source_name = saved["clip"]["sourceName"].as_str().unwrap_or(&source.name).to_string();
        Some(Asset {
            rigs: Vec::new(),
            id: saved["id"].as_str().map(String::from).unwrap_or_else(|| format!("uid://anim_{key:016x}_{index}")), name: name.clone(), file_name: source.file_name.clone(),
            rel_path: source.rel_path.clone(), pack: source.pack.clone(), category: source.category.clone(),
            content: AssetContent::Animation { fileset: fileset.clone(), clip: ClipMeta { tracks, standalone, index, name, duration,
                source_id, source_name, target_nodes: targets, rig } },
            variants: Vec::new(), thumb: ThumbMeta::default(), user: UserMeta {provenance:source.user.provenance.clone(),tags:source.user.tags.clone(),..UserMeta::default()}, tags: source.tags.clone(), origin: source.origin.clone(), format: source.format.clone(), content_hash: None,
        })
    }).collect()
}

pub fn select_clip(mut doc: Value, index: usize) -> AppResult<Value> {
    let clip = doc["animations"].as_array().and_then(|clips| clips.get(index)).cloned()
        .ok_or_else(|| AppError::msg(format!("animation clip {index} is missing from its source")))?;
    if !clip["channels"].as_array().is_some_and(|channels| !channels.is_empty()) {
        return Err(AppError::msg("animation clip has no channels"));
    }
    doc["animations"] = Value::Array(vec![clip]);
    Ok(doc)
}

pub fn export_stem(asset: &Asset) -> String {
    let AssetContent::Animation { clip, .. } = &asset.content else { return asset.name.clone() };
    let source: String = clip.source_name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).take(100).collect();
    let safe: String = clip.name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).take(80).collect();
    format!("{source}_{safe}_{}", clip.index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn source() -> Asset {
        serde_json::from_value(json!({"id":"hero","name":"Hero","fileName":"Hero.gltf","relPath":"library/p/characters/Hero.gltf","pack":"p","category":"characters","kind":"model","fileset":{"gltf":"library/p/characters/Hero.gltf","bin":"library/p/characters/Hero.bin","textures":[]},"animation":{"clipCount":1,"clipNames":["Walk"]},"thumb":{"state":"missing"},"user":{"favorite":false,"tags":[]}})).unwrap()
    }
    fn doc() -> Value { json!({"nodes":[{"name":"Root"}],"accessors":[{"max":[1.25]}],"animations":[{"name":"Empty","channels":[]},{"name":"Walk/Forward","samplers":[{"input":0,"output":1}],"channels":[{"sampler":0,"target":{"node":0,"path":"translation"}}]}]}) }

    #[test]
    fn indexes_original_array_skipping_empty_clips_without_renumbering() {
        let source = source(); let assets = clips(&doc(), &source, source.fileset().unwrap());
        assert_eq!(assets.len(), 1);
        let AssetContent::Animation { clip, .. } = &assets[0].content else { panic!("clip expected") };
        assert_eq!(clip.index, 1); assert_eq!(clip.duration, 1.25); assert_eq!(clip.target_nodes, vec!["Root"]);
        assert!(!export_stem(&assets[0]).contains('/'));
        let mut renamed = source.clone(); renamed.id = "other-import-id".into();
        assert_eq!(assets[0].id, clips(&doc(), &renamed, renamed.fileset().unwrap())[0].id);
    }
    #[test]
    fn clips_of_a_source_file_carry_its_library_tags() {
        let mut source = source(); source.tags = vec!["walk".into(), "loop".into()];
        let assets = clips(&doc(), &source, source.fileset().unwrap());
        assert_eq!(assets[0].tags, vec!["walk", "loop"]);
    }
    #[test]
    fn single_clip_export_keeps_only_the_chosen_animation_and_preserves_source_data() {
        let source = doc(); let selected = select_clip(source.clone(), 1).unwrap();
        assert_eq!(selected["animations"].as_array().unwrap().len(), 1);
        assert_eq!(selected["animations"][0]["name"], "Walk/Forward");
        assert_eq!(selected["nodes"], source["nodes"]); assert_eq!(selected["accessors"], source["accessors"]);
        assert!(select_clip(source.clone(), 0).is_err()); assert!(select_clip(source, 3).is_err());
    }
    #[test]
    fn malformed_clip_times_are_not_advertised_as_usable() {
        let mut invalid = doc(); invalid["accessors"][0]["max"] = json!([-1]);
        let source = source(); assert!(clips(&invalid, &source, source.fileset().unwrap()).is_empty());
    }
}
