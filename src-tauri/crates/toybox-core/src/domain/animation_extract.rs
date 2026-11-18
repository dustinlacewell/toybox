//! Standalone animation glTF: node/rest hierarchy and sampled tracks only.
//! No mesh, texture, skin weights, inverse-bind data or source-file references.
use crate::{
    domain::{animation, rig},
    error::{AppError, AppResult},
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub struct Extracted {
    pub doc: Value,
    pub bin: Vec<u8>,
}

pub fn extract(
    source: &Value,
    buffers: &[Vec<u8>],
    index: usize,
    bin_name: &str,
    metadata: Value,
) -> AppResult<Extracted> {
    let selected = animation::select_clip(source.clone(), index)?;
    let mut clip = selected["animations"][0].clone();
    for channel in clip["channels"]
        .as_array()
        .ok_or_else(|| AppError::msg("Missing animation channels"))?
    {
        if channel["target"]["path"].as_str() == Some("weights") {
            return Err(AppError::msg("Morph-target animation extraction needs a target mesh schema; import skeletal or node-transform clips instead."));
        }
        if !matches!(
            channel["target"]["path"].as_str(),
            Some("translation" | "rotation" | "scale")
        ) {
            return Err(AppError::msg("Unsupported animation target property"));
        }
    }
    let (_, binding) =
        rig::describe(source).ok_or_else(|| AppError::msg("Invalid animation node hierarchy"))?;
    let mut included: Vec<_> = binding.nodes.values().copied().collect();
    included.sort_unstable();
    included.dedup();
    // Duplicate names are still extractable; they simply cannot auto-match.
    // Include all nodes for these files so no target disappears.
    if !rig::describe(source).unwrap().0.unambiguous {
        included = (0..source["nodes"].as_array().unwrap().len()).collect();
    }
    let node_map: BTreeMap<_, _> = included
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new))
        .collect();
    let mut nodes = Vec::new();
    let mut children = std::collections::BTreeSet::new();
    for old in &included {
        let node = &source["nodes"][*old];
        let mut clean = json!({});
        for field in ["name", "translation", "rotation", "scale", "matrix"] {
            if let Some(value) = node.get(field) {
                clean[field] = value.clone();
            }
        }
        let links: Vec<_> = node["children"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|child| {
                child
                    .as_u64()
                    .and_then(|index| node_map.get(&(index as usize)))
                    .copied()
            })
            .collect();
        children.extend(links.iter().copied());
        if !links.is_empty() {
            clean["children"] = json!(links);
        }
        nodes.push(clean);
    }
    for channel in clip["channels"].as_array_mut().unwrap() {
        let old = channel["target"]["node"]
            .as_u64()
            .ok_or_else(|| AppError::msg("Animation target node is missing"))?
            as usize;
        channel["target"]["node"] = json!(node_map
            .get(&old)
            .ok_or_else(|| AppError::msg("Animation target is outside its rig"))?);
    }
    let mut accessor_map = BTreeMap::new();
    let mut view_map = BTreeMap::new();
    let mut accessors = Vec::new();
    let mut views = Vec::new();
    let mut bin = Vec::new();
    for sampler in clip["samplers"]
        .as_array_mut()
        .ok_or_else(|| AppError::msg("Animation samplers are missing"))?
    {
        for field in ["input", "output"] {
            let old = sampler[field]
                .as_u64()
                .ok_or_else(|| AppError::msg("Missing sampler accessor"))?
                as usize;
            let new = if let Some(index) = accessor_map.get(&old) {
                *index
            } else {
                let mut accessor = source["accessors"]
                    .get(old)
                    .cloned()
                    .ok_or_else(|| AppError::msg("Animation accessor is missing"))?;
                let mut remap_view = |slot: &mut Value| -> AppResult<()> {
                    let Some(old) = slot.as_u64() else {
                        return Ok(());
                    };
                    let old = old as usize;
                    let new = if let Some(index) = view_map.get(&old) {
                        *index
                    } else {
                        let mut view = source["bufferViews"]
                            .get(old)
                            .cloned()
                            .ok_or_else(|| AppError::msg("Missing animation buffer view"))?;
                        let buffer = view["buffer"]
                            .as_u64()
                            .and_then(|i| buffers.get(i as usize))
                            .ok_or_else(|| AppError::msg("Missing animation buffer"))?;
                        let start = usize::try_from(view["byteOffset"].as_u64().unwrap_or(0))
                            .map_err(|_| AppError::msg("Invalid buffer offset"))?;
                        let size = usize::try_from(
                            view["byteLength"]
                                .as_u64()
                                .ok_or_else(|| AppError::msg("Missing buffer length"))?,
                        )
                        .map_err(|_| AppError::msg("Invalid buffer length"))?;
                        let end = start
                            .checked_add(size)
                            .ok_or_else(|| AppError::msg("Animation buffer overflow"))?;
                        let bytes = buffer
                            .get(start..end)
                            .ok_or_else(|| AppError::msg("Truncated animation buffer"))?;
                        while bin.len() % 4 != 0 {
                            bin.push(0);
                        }
                        view["buffer"] = json!(0);
                        view["byteOffset"] = json!(bin.len());
                        view.as_object_mut().unwrap().remove("target");
                        bin.extend_from_slice(bytes);
                        let index = views.len();
                        views.push(view);
                        view_map.insert(old, index);
                        index
                    };
                    *slot = json!(new);
                    Ok(())
                };
                if accessor.get("bufferView").is_some() {
                    remap_view(&mut accessor["bufferView"])?;
                }
                if accessor.get("sparse").is_some() {
                    remap_view(&mut accessor["sparse"]["indices"]["bufferView"])?;
                    remap_view(&mut accessor["sparse"]["values"]["bufferView"])?;
                }
                let index = accessors.len();
                accessors.push(accessor);
                accessor_map.insert(old, index);
                index
            };
            sampler[field] = json!(new);
        }
    }
    let mut joints: Vec<_> = rig::joint_indices(source)
        .into_iter()
        .filter_map(|index| node_map.get(&index).copied())
        .collect();
    joints.sort_unstable();
    joints.dedup();
    let mut metadata = metadata;
    metadata["joints"] = json!(joints);
    let roots: Vec<_> = (0..nodes.len())
        .filter(|index| !children.contains(index))
        .collect();
    let doc = json!({"asset":{"version":"2.0","generator":"Toybox animation extraction"},"scene":0,"scenes":[{"nodes":roots}],"nodes":nodes,
        "animations":[clip],"accessors":accessors,"bufferViews":views,"buffers":[{"uri":bin_name,"byteLength":bin.len()}],"extras":{"toyboxAnimation":metadata}});
    Ok(Extracted { doc, bin })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extraction_compacts_samples_and_removes_every_model_dependency() {
        let doc = json!({"asset":{"version":"2.0"},"nodes":[{"name":"Root","children":[1]},{"name":"Hip","translation":[0,1,0]},{"name":"Body","mesh":0,"skin":0}],"skins":[{"joints":[0,1],"inverseBindMatrices":2}],"meshes":[{}],"images":[{"uri":"atlas.png"}],"buffers":[{"uri":"Hero.bin","byteLength":128}],"bufferViews":[{"buffer":0,"byteOffset":64,"byteLength":8},{"buffer":0,"byteOffset":72,"byteLength":24}],"accessors":[{"bufferView":0,"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[1]},{"bufferView":1,"componentType":5126,"count":2,"type":"VEC3"}],"animations":[{"name":"Walk","samplers":[{"input":0,"output":1}],"channels":[{"sampler":0,"target":{"node":1,"path":"translation"}}]}]});
        let bytes: Vec<_> = (0..128).collect();
        let result = extract(&doc, &[bytes.clone()], 0, "Walk.bin", json!({"id":"walk"})).unwrap();
        assert_eq!(result.bin, bytes[64..96]);
        for field in ["meshes", "skins", "images", "textures", "materials"] {
            assert!(result.doc.get(field).is_none());
        }
        assert_eq!(result.doc["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(
            result.doc["animations"][0]["channels"][0]["target"]["node"],
            1
        );
        assert_eq!(
            rig::describe(&doc).unwrap().0.id,
            rig::describe(&result.doc).unwrap().0.id
        );
        let mut morph = doc.clone();
        morph["animations"][0]["channels"][0]["target"]["path"] = json!("weights");
        assert!(extract(&morph, &[bytes], 0, "Walk.bin", json!({})).is_err());
    }
}
