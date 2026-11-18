//! Named topology and rest frames. File-local node indices never identify a rig.
use crate::domain::mat4;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigNode {
    pub key: String,
    pub name: String,
    pub parent: Option<String>,
    pub rest: [f64; 16],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rig {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub unambiguous: bool,
    pub nodes: Vec<RigNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigBinding {
    pub rig_id: String,
    pub nodes: BTreeMap<String, usize>,
}

pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ *byte as u64).wrapping_mul(0x100000001b3)
    })
}

pub fn rest(node: &Value) -> [f64; 16] {
    if let Some(matrix) = node["matrix"]
        .as_array()
        .filter(|matrix| matrix.len() == 16)
    {
        return std::array::from_fn(|i| matrix[i].as_f64().unwrap_or(f64::NAN));
    }
    let t = std::array::from_fn(|i| node["translation"][i].as_f64().unwrap_or(0.0));
    let s = std::array::from_fn(|i| node["scale"][i].as_f64().unwrap_or(1.0));
    let mut q: [f64; 4] = std::array::from_fn(|i| {
        node["rotation"][i]
            .as_f64()
            .unwrap_or(if i == 3 { 1.0 } else { 0.0 })
    });
    let length = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    if length > 0.0 {
        for value in &mut q {
            *value /= length;
        }
    }
    mat4::trs(t, q, s)
}

/// Meshless animation exports may retain skins without node.skin users.
pub fn joint_indices(doc: &Value) -> BTreeSet<usize> {
    let mut skins: BTreeSet<_> = doc["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|node| node["skin"].as_u64())
        .map(|i| i as usize)
        .collect();
    if skins.is_empty()
        && doc["meshes"]
            .as_array()
            .is_none_or(|meshes| meshes.is_empty())
    {
        skins.extend(0..doc["skins"].as_array().map_or(0, Vec::len));
    }
    let mut joints: BTreeSet<_> = skins
        .into_iter()
        .flat_map(|index| {
            doc["skins"][index]["joints"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .filter_map(Value::as_u64)
        .map(|i| i as usize)
        .collect();
    joints.extend(
        doc["extras"]["toyboxAnimation"]["joints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .map(|i| i as usize),
    );
    joints
}

/// Union the used skins and their ancestors. Unused export debris is excluded.
/// Transform-only objects get a node profile, kept separate from skeletons.
pub fn describe(doc: &Value) -> Option<(Rig, RigBinding)> {
    let nodes = doc["nodes"].as_array()?;
    if nodes.is_empty() {
        return None;
    }
    let mut parents = vec![None; nodes.len()];
    for (parent, node) in nodes.iter().enumerate() {
        for child in node["children"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
        {
            let child = child as usize;
            if child >= nodes.len() || parents[child].is_some() {
                return None;
            }
            parents[child] = Some(parent);
        }
    }
    let mut included = joint_indices(doc);
    let skeletal = !included.is_empty();
    if skeletal {
        // Non-joint roots may carry locomotion or the coordinate conversion.
        for animation in doc["animations"].as_array().into_iter().flatten() {
            for channel in animation["channels"].as_array().into_iter().flatten() {
                if let Some(index) = channel["target"]["node"].as_u64() {
                    included.insert(index as usize);
                }
            }
        }
    } else {
        included.extend(0..nodes.len());
    }
    for index in included.clone() {
        let mut current = index;
        let mut visited = BTreeSet::new();
        loop {
            if current >= nodes.len() || !visited.insert(current) {
                return None;
            }
            included.insert(current);
            match parents[current] {
                Some(parent) => current = parent,
                None => break,
            }
        }
    }
    let key_for = |index: usize| -> Option<String> {
        let mut chain = vec![];
        let mut current = Some(index);
        let mut visited = BTreeSet::new();
        while let Some(index) = current {
            if !visited.insert(index) {
                return None;
            }
            let name = nodes[index]["name"].as_str().unwrap_or("");
            chain.push(name.replace('~', "~0").replace('/', "~1"));
            current = parents[index];
        }
        chain.reverse();
        Some(chain.join("/"))
    };
    let mut binding = BTreeMap::new();
    let mut definitions = Vec::new();
    let mut unambiguous = true;
    for &index in &included {
        let key = key_for(index)?;
        let name = nodes[index]["name"].as_str().unwrap_or("").to_string();
        let matrix = rest(&nodes[index]);
        if name.is_empty()
            || matrix.iter().any(|v| !v.is_finite())
            || binding.insert(key.clone(), index).is_some()
        {
            unambiguous = false;
        }
        definitions.push(RigNode {
            key,
            name,
            parent: parents[index].map(&key_for).flatten(),
            rest: matrix,
        });
    }
    definitions.sort_by(|a, b| a.key.cmp(&b.key));
    // Quantize only identity, never the authored transforms used for playback.
    let identity: Vec<_> = definitions
        .iter()
        .map(|node| {
            (
                &node.key,
                &node.parent,
                node.rest.map(|v| (v * 100_000.0).round() as i64),
            )
        })
        .collect();
    let kind = if skeletal { "skeleton" } else { "nodes" }.to_string();
    let id = format!(
        "rig_{:016x}",
        hash(&serde_json::to_vec(&(&kind, &identity)).ok()?)
    );
    let name = definitions
        .iter()
        .find(|node| node.parent.is_none())
        .map(|node| node.name.clone())
        .unwrap_or_else(|| "Rig".into());
    Some((
        Rig {
            id: id.clone(),
            name,
            kind,
            unambiguous,
            nodes: definitions,
        },
        RigBinding {
            rig_id: id,
            nodes: binding,
        },
    ))
}

/// A fingerprint of a rig's named topology without its rest frames: rigs that
/// can be `equivalent` always share it, so it narrows a lookup in a database.
pub fn shape(rig: &Rig) -> String {
    let mut text = format!("{}|{}", rig.kind, rig.nodes.len());
    for node in &rig.nodes {
        text.push('|');
        text.push_str(&node.key);
        text.push('<');
        text.push_str(node.parent.as_deref().unwrap_or(""));
    }
    format!("{}:{:016x}", rig.nodes.len(), hash(text.as_bytes()))
}

/// Exact named topology and rest-frame match, with conversion noise tolerated.
pub fn equivalent(a: &Rig, b: &Rig) -> bool {
    a.unambiguous
        && b.unambiguous
        && a.kind == b.kind
        && a.nodes.len() == b.nodes.len()
        && a.nodes.iter().zip(&b.nodes).all(|(a, b)| {
            a.key == b.key
                && a.parent == b.parent
                && a.rest
                    .iter()
                    .zip(b.rest)
                    .all(|(a, b)| (a - b).abs() <= 0.00001)
        })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub rig_id: String,
    pub exact: bool,
    pub nodes: BTreeMap<usize, usize>,
}

pub fn match_animation(
    animation: &crate::domain::catalog_model::Asset,
    model: &crate::domain::catalog_model::Asset,
    registry: &[Rig],
) -> Option<Match> {
    use crate::domain::catalog_model::AssetContent;
    let AssetContent::Animation { clip, .. } = &animation.content else {
        return None;
    };
    if !matches!(model.content, AssetContent::Model { .. }) {
        return None;
    }
    match_tracks(&clip.tracks, &animation.rigs, &model.rigs, registry)
}

/// Shared by catalog suggestions and validation of the current source files.
pub fn match_tracks(
    tracks: &[crate::domain::animation::TrackTarget],
    sources: &[RigBinding],
    targets: &[RigBinding],
    registry: &[Rig],
) -> Option<Match> {
    if tracks.is_empty()
        || tracks
            .iter()
            .any(|track| !matches!(track.path.as_str(), "translation" | "rotation" | "scale"))
    {
        return None;
    }
    for source in sources {
        let a = registry.iter().find(|rig| rig.id == source.rig_id)?;
        if !a.unambiguous {
            continue;
        }
        for target in targets {
            let Some(b) = registry.iter().find(|rig| rig.id == target.rig_id) else {
                continue;
            };
            if !b.unambiguous || a.kind != b.kind {
                continue;
            }
            let mut mapping = BTreeMap::new();
            let mut valid = true;
            for track in tracks {
                let Some((key, _)) = source.nodes.iter().find(|(_, index)| **index == track.node)
                else {
                    valid = false;
                    break;
                };
                let Some(&index) = target.nodes.get(key) else {
                    valid = false;
                    break;
                };
                mapping.insert(track.node, index);
                let mut current = Some(key.as_str());
                while let Some(key) = current {
                    let Some(an) = a.nodes.iter().find(|node| node.key == key) else {
                        valid = false;
                        break;
                    };
                    let Some(bn) = b.nodes.iter().find(|node| node.key == key) else {
                        valid = false;
                        break;
                    };
                    if an.parent != bn.parent
                        || !an
                            .rest
                            .iter()
                            .zip(bn.rest)
                            .all(|(a, b)| (a - b).abs() <= 0.00001)
                    {
                        valid = false;
                        break;
                    }
                    current = an.parent.as_deref();
                }
                if !valid {
                    break;
                }
            }
            if valid {
                return Some(Match {
                    rig_id: target.rig_id.clone(),
                    exact: equivalent(a, b),
                    nodes: mapping,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn meshless_animation_retains_unbound_skin_joint_identity() {
        let doc = json!({"nodes":[{"name":"Root","children":[1]},{"name":"Hip"}],"skins":[{"joints":[0,1]}]});
        assert_eq!(joint_indices(&doc), BTreeSet::from([0, 1]));
        assert_eq!(describe(&doc).unwrap().0.kind, "skeleton");
    }
    #[test]
    fn identity_ignores_file_indices_skin_names_and_transform_representation() {
        let a = json!({"nodes":[{"name":"Root","children":[1]},{"name":"Hip","translation":[0,1,0]},{"name":"Body","skin":0}],"skins":[{"name":"A","joints":[0,1]}]});
        let mut b = json!({"nodes":[{"name":"Body","skin":0},{"name":"Hip","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]},{"name":"Root","children":[1]}],"skins":[{"name":"B","joints":[1,2]}]});
        let (ra, ba) = describe(&a).unwrap();
        let (rb, bb) = describe(&b).unwrap();
        assert_eq!(ra.id, rb.id);
        assert!(equivalent(&ra, &rb));
        assert_ne!(ba.nodes["Root"], bb.nodes["Root"]);
        b["nodes"][1]["matrix"][13] = json!(2);
        assert!(!equivalent(&ra, &describe(&b).unwrap().0));
        b["nodes"][1]["name"] = json!("Other");
        assert!(!equivalent(&ra, &describe(&b).unwrap().0));
    }
    #[test]
    fn duplicate_sibling_names_cannot_match_automatically() {
        let d = json!({"nodes":[{"name":"Root","children":[1,2]},{"name":"Bone"},{"name":"Bone"}]});
        let (rig, _) = describe(&d).unwrap();
        assert!(!rig.unambiguous);
        assert!(!equivalent(&rig, &rig));
    }
    #[test]
    fn matching_requires_names_hierarchy_and_rest_but_allows_unanimated_extra_bones() {
        use crate::domain::catalog_model::Asset;
        let source = json!({"nodes":[{"name":"Root","children":[1]},{"name":"Hip","translation":[0,1,0]},{"name":"Body","skin":0}],"skins":[{"joints":[0,1]}]});
        let (rig, binding) = describe(&source).unwrap();
        let base = json!({"id":"model","name":"Body","fileName":"Body.gltf","relPath":"Body.gltf","pack":"p","category":"characters","kind":"model","fileset":{"gltf":"Body.gltf","bin":"Body.bin","textures":[]},"rigs":[binding],"user":{"favorite":false,"tags":[]},"thumb":{"state":"missing"}});
        let model: Asset = serde_json::from_value(base.clone()).unwrap();
        let mut clip = base;
        clip["kind"] = json!("animation");
        clip["id"] = json!("walk");
        clip["clip"] = json!({"standalone":true,"index":0,"name":"Walk","duration":1,"sourceId":"model","sourceName":"Body","targetNodes":["Hip"],"tracks":[{"node":1,"path":"rotation"}],"rig":"Root"});
        let animation: Asset = serde_json::from_value(clip).unwrap();
        assert!(
            match_animation(&animation, &model, &[rig.clone()])
                .unwrap()
                .exact
        );
        let mut candidate = source.clone();
        candidate["nodes"][0]["children"] = json!([1, 3]);
        candidate["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"Accessory","translation":[9,0,0]}));
        candidate["skins"][0]["joints"] = json!([0, 1, 3]);
        let (extra, binding) = describe(&candidate).unwrap();
        let mut other = model.clone();
        other.rigs = vec![binding];
        assert!(
            !match_animation(&animation, &other, &[rig.clone(), extra])
                .unwrap()
                .exact
        );
        candidate["nodes"][1]["translation"] = json!([0, 2, 0]);
        let (changed, binding) = describe(&candidate).unwrap();
        other.rigs = vec![binding];
        assert!(match_animation(&animation, &other, &[rig.clone(), changed]).is_none());
        candidate["nodes"][1]["name"] = json!("Spine");
        let (renamed, binding) = describe(&candidate).unwrap();
        other.rigs = vec![binding];
        assert!(match_animation(&animation, &other, &[rig, renamed]).is_none());
    }
}
