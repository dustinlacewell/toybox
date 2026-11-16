//! Compose a temporary preview; it never changes the model or animation asset.
use crate::error::{AppError, AppResult};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn compose(
    mut model: Value,
    animation: &Value,
    mapping: &BTreeMap<usize, usize>,
) -> AppResult<Value> {
    let mut clip = animation["animations"][0].clone();
    if clip.is_null() {
        return Err(AppError::msg("Preview animation is missing"));
    }
    let buffer_offset = model["buffers"].as_array().map_or(0, Vec::len);
    let view_offset = model["bufferViews"].as_array().map_or(0, Vec::len);
    let accessor_offset = model["accessors"].as_array().map_or(0, Vec::len);
    let mut views = animation["bufferViews"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for view in &mut views {
        view["buffer"] = json!(
            view["buffer"]
                .as_u64()
                .ok_or_else(|| AppError::msg("Invalid preview buffer"))? as usize
                + buffer_offset
        );
    }
    let mut accessors = animation["accessors"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for accessor in &mut accessors {
        if let Some(view) = accessor["bufferView"].as_u64() {
            accessor["bufferView"] = json!(view as usize + view_offset);
        }
        if accessor.get("sparse").is_some() {
            for field in ["indices", "values"] {
                let view = accessor["sparse"][field]["bufferView"]
                    .as_u64()
                    .ok_or_else(|| AppError::msg("Invalid sparse animation accessor"))?;
                accessor["sparse"][field]["bufferView"] = json!(view as usize + view_offset);
            }
        }
    }
    for sampler in clip["samplers"]
        .as_array_mut()
        .ok_or_else(|| AppError::msg("Missing preview samplers"))?
    {
        for field in ["input", "output"] {
            sampler[field] = json!(
                sampler[field]
                    .as_u64()
                    .ok_or_else(|| AppError::msg("Invalid preview sampler"))?
                    as usize
                    + accessor_offset
            );
        }
    }
    for channel in clip["channels"]
        .as_array_mut()
        .ok_or_else(|| AppError::msg("Missing preview channels"))?
    {
        let old = channel["target"]["node"]
            .as_u64()
            .ok_or_else(|| AppError::msg("Invalid preview target"))? as usize;
        channel["target"]["node"] = json!(mapping
            .get(&old)
            .ok_or_else(|| AppError::msg("Animation does not match this model"))?);
    }
    for (field, items) in [
        (
            "buffers",
            animation["buffers"].as_array().cloned().unwrap_or_default(),
        ),
        ("bufferViews", views),
        ("accessors", accessors),
    ] {
        if model.get(field).is_none() {
            model[field] = json!([]);
        }
        model[field]
            .as_array_mut()
            .ok_or_else(|| AppError::msg("Invalid preview document"))?
            .extend(items);
    }
    model["animations"] = json!([clip]);
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_remaps_sample_buffers_sparse_views_and_node_indices_without_modifying_model_geometry(
    ) {
        let model = json!({"nodes":[{"name":"Body","mesh":0},{"name":"Root"}],"meshes":[{"primitives":[]}],"buffers":[{"uri":"model.bin"}],"bufferViews":[{"buffer":0,"byteLength":4}],"accessors":[{"bufferView":0}],"images":[{"uri":"atlas.png"}]});
        let clip = json!({"buffers":[{"uri":"clip.bin"}],"bufferViews":[{"buffer":0,"byteLength":8},{"buffer":0,"byteLength":24}],"accessors":[{"bufferView":0},{"bufferView":1,"sparse":{"indices":{"bufferView":0},"values":{"bufferView":1}}}],"animations":[{"samplers":[{"input":0,"output":1}],"channels":[{"sampler":0,"target":{"node":0,"path":"translation"}}]}]});
        let result = compose(model.clone(), &clip, &BTreeMap::from([(0, 1)])).unwrap();
        assert_eq!(result["meshes"], model["meshes"]);
        assert_eq!(result["images"], model["images"]);
        assert_eq!(result["bufferViews"][1]["buffer"], 1);
        assert_eq!(result["accessors"][2]["sparse"]["values"]["bufferView"], 2);
        assert_eq!(result["animations"][0]["samplers"][0]["output"], 2);
        assert_eq!(result["animations"][0]["channels"][0]["target"]["node"], 1);
        assert!(compose(model, &clip, &BTreeMap::new()).is_err());
    }
}
