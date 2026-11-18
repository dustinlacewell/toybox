//! Preview source reads avoid deserializing the entire catalog per thumbnail.
//! Canonical filesystem containment preserves the library read boundary.
use crate::{
    domain::{animation, gltf_parse, gltf_uri, gltf_vertex_colors, paths, rig},
    error::{AppError, AppResult},
    infra::fsio,
};
use serde_json::Value;
use std::path::Path;

fn under(root: &Path, path: &Path) -> AppResult<std::path::PathBuf> {
    let canonical = path
        .canonicalize()
        .map_err(|error| AppError::file("Resolve", path, error))?;
    if !canonical.starts_with(root) {
        return Err(AppError::msg(
            "Preview source is outside the selected library",
        ));
    }
    Ok(canonical)
}

pub fn read_source(
    root: &Path,
    relative_path: &str,
    clip_index: Option<usize>,
) -> AppResult<Value> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| AppError::file("Resolve library", root, error))?;
    if Path::new(relative_path).is_absolute()
        || Path::new(relative_path)
            .extension()
            .and_then(|ext| ext.to_str())
            != Some("gltf")
    {
        return Err(AppError::msg(
            "Preview source must be a library-relative glTF file",
        ));
    }
    let path = under(&canonical_root, &root.join(relative_path))?;
    let mut doc = gltf_parse::parse(&fsio::read_text(&path)?)?;
    if gltf_vertex_colors::may_need_repair(&doc) {
        let uri = doc["buffers"][0]["uri"]
            .as_str()
            .ok_or_else(|| AppError::msg("Legacy preview needs an external binary buffer"))?;
        let bytes = match gltf_uri::decode_data_uri(uri) {
            Some((_, bytes)) => bytes,
            None if gltf_uri::external_file(uri).is_none() => {
                return Err(AppError::msg("Legacy preview needs an external binary buffer"))
            }
            None => fsio::read_bytes(&under(&canonical_root, &path.parent().unwrap().join(uri))?)?,
        };
        gltf_vertex_colors::repair(&mut doc, &[bytes]);
    }
    match clip_index {
        Some(index) => animation::select_clip(doc, index),
        None => {
            doc.as_object_mut()
                .ok_or_else(|| AppError::msg("Preview source must be a glTF JSON object"))?
                .remove("animations");
            Ok(doc)
        }
    }
}

/// Revalidate live topology and compose only the two selected files. Neither
/// thumbnail rendering nor playback needs to reload the library catalog.
pub fn read_pair(
    root: &Path,
    animation_path: &str,
    clip_index: usize,
    model_path: &str,
) -> AppResult<Value> {
    let mut animation = read_source(root, animation_path, Some(clip_index))?;
    let mut model = read_source(root, model_path, None)?;
    let (a, ab) =
        rig::describe(&animation).ok_or_else(|| AppError::msg("Invalid animation hierarchy"))?;
    let (b, bb) = rig::describe(&model).ok_or_else(|| AppError::msg("Invalid model hierarchy"))?;
    let tracks: Vec<animation::TrackTarget> = animation["animations"][0]["channels"]
        .as_array()
        .ok_or_else(|| AppError::msg("Missing animation channels"))?
        .iter()
        .map(|channel| serde_json::from_value(channel["target"].clone()))
        .collect::<Result<_, _>>()?;
    let mapping = rig::match_tracks(&tracks, &[ab], &[bb], &[a,b]).ok_or_else(|| AppError::msg("The animation and model do not have compatible named hierarchies and rest transforms. Rescan if either file changed."))?;
    for (doc, path) in [(&mut animation, animation_path), (&mut model, model_path)] {
        let normalized = path.replace('\\', "/");
        let parent = normalized.rsplit_once('/').map_or("", |(parent, _)| parent);
        for field in ["buffers", "images"] {
            if let Some(items) = doc.get_mut(field).and_then(Value::as_array_mut) {
                for item in items {
                    if let Some(uri) = item["uri"].as_str().and_then(gltf_uri::external_file)
                    {
                        item["uri"] = Value::String(format!(
                            "toybox-resource:{}",
                            paths::resolve_uri_rel(parent, uri)
                        ));
                    }
                }
            }
        }
    }
    crate::domain::animation_preview::compose(model, &animation, &mapping.nodes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn pair_rechecks_live_targets_and_rejects_changed_rest_frames_without_a_catalog() {
        let folder =
            std::env::temp_dir().join(format!("toybox-preview-pair-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let animation = json!({"asset":{"version":"2.0"},"nodes":[{"name":"Root"}],"buffers":[{"uri":"clip.bin"}],"accessors":[{},{}],"animations":[{"samplers":[{"input":0,"output":1}],"channels":[{"sampler":0,"target":{"node":0,"path":"translation"}}]}]});
        let mut model = json!({"asset":{"version":"2.0"},"nodes":[{"name":"Root"}],"buffers":[{"uri":"body.bin"}]});
        std::fs::write(
            folder.join("Clip.gltf"),
            serde_json::to_vec(&animation).unwrap(),
        )
        .unwrap();
        std::fs::write(
            folder.join("Model.gltf"),
            serde_json::to_vec(&model).unwrap(),
        )
        .unwrap();
        let result = read_pair(&folder, "Clip.gltf", 0, "Model.gltf").unwrap();
        assert_eq!(result["buffers"][1]["uri"], "toybox-resource:clip.bin");
        model["nodes"][0]["translation"] = json!([1, 0, 0]);
        std::fs::write(
            folder.join("Model.gltf"),
            serde_json::to_vec(&model).unwrap(),
        )
        .unwrap();
        assert!(read_pair(&folder, "Clip.gltf", 0, "Model.gltf").is_err());
        assert!(read_pair(&folder, "../Outside.gltf", 0, "Model.gltf").is_err());
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn reads_without_a_catalog_selects_clips_and_rejects_parent_escape() {
        let folder =
            std::env::temp_dir().join(format!("toybox-preview-source-{}", std::process::id()));
        let root = folder.join("library");
        std::fs::create_dir_all(&root).unwrap();
        let doc = json!({"asset":{"version":"2.0"},"nodes":[{"name":"Root"}],"animations":[{"name":"Walk","channels":[{}]},{"name":"Run","channels":[{}]}]});
        std::fs::write(root.join("Hero.gltf"), serde_json::to_vec(&doc).unwrap()).unwrap();
        std::fs::write(folder.join("Outside.gltf"), b"{}").unwrap();
        assert!(read_source(&root, "Hero.gltf", None)
            .unwrap()
            .get("animations")
            .is_none());
        assert_eq!(
            read_source(&root, "Hero.gltf", Some(1)).unwrap()["animations"][0]["name"],
            "Run"
        );
        assert!(read_source(&root, "../Outside.gltf", None).is_err());
        assert!(read_source(&root, "Missing.gltf", None)
            .unwrap_err()
            .to_string()
            .contains("Missing.gltf"));
        std::fs::remove_dir_all(folder).unwrap();
    }
}
