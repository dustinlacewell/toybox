//! Origin-correction for "shoddy" assets whose geometry sits far from their
//! local origin (the point a game engine rotates/scales around). We compute the
//! asset's world-space bounding box (composing node transforms), then bake a
//! translation that lands a chosen BB point (min/center/max per axis) on the
//! origin — applied as a transform on the scene root so it is correct for static
//! AND skinned meshes (no vertex/.bin rewrite).
//!
//! All math here is pure; the command layer supplies the parsed glTF and writes
//! the result back.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::mat4::{self, Mat4, Vec3};
use crate::error::{AppError, AppResult};

/// Which axis a correction targets.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }
}

/// Which point along the axis to bring to the origin.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Min,
    Center,
    Max,
}

/// A world-space axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    fn empty() -> Self {
        Aabb {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        }
    }
    fn expand(&mut self, p: Vec3) {
        for i in 0..3 {
            if p[i] < self.min[i] {
                self.min[i] = p[i];
            }
            if p[i] > self.max[i] {
                self.max[i] = p[i];
            }
        }
    }
    fn is_valid(&self) -> bool {
        self.min[0] <= self.max[0]
    }
    /// The coordinate of the chosen alignment point on the given axis.
    fn align_coord(&self, axis: Axis, align: Align) -> f64 {
        let i = axis.index();
        match align {
            Align::Min => self.min[i],
            Align::Max => self.max[i],
            Align::Center => 0.5 * (self.min[i] + self.max[i]),
        }
    }
}

/// Compute the asset's world-space AABB by composing node transforms from the
/// scene root and unioning each mesh primitive's accessor min/max corners.
pub fn world_aabb(doc: &Value) -> AppResult<Aabb> {
    let scene = active_scene(doc)?;
    let roots = scene
        .get("nodes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::msg("scene has no nodes"))?;

    let mut aabb = Aabb::empty();
    for root in roots {
        let idx = root.as_u64().ok_or_else(|| AppError::msg("bad node index"))? as usize;
        accumulate_node(doc, idx, &mat4::IDENTITY, &mut aabb)?;
    }
    if !aabb.is_valid() {
        return Err(AppError::msg("asset has no geometry to bound"));
    }
    Ok(aabb)
}

/// Recursively accumulate a node's (and descendants') mesh corners into `aabb`.
fn accumulate_node(doc: &Value, node_idx: usize, parent: &Mat4, aabb: &mut Aabb) -> AppResult<()> {
    let node = node_at(doc, node_idx)?;
    let local = node_local_matrix(node)?;
    let world = mat4::mul(parent, &local);

    if let Some(mesh_idx) = node.get("mesh").and_then(|v| v.as_u64()) {
        accumulate_mesh(doc, mesh_idx as usize, &world, aabb)?;
    }

    if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
        for c in children {
            let ci = c.as_u64().ok_or_else(|| AppError::msg("bad child index"))? as usize;
            accumulate_node(doc, ci, &world, aabb)?;
        }
    }
    Ok(())
}

/// Union the 8 transformed corners of every primitive's POSITION accessor bounds.
fn accumulate_mesh(doc: &Value, mesh_idx: usize, world: &Mat4, aabb: &mut Aabb) -> AppResult<()> {
    let primitives = doc
        .get("meshes")
        .and_then(|m| m.as_array())
        .and_then(|m| m.get(mesh_idx))
        .and_then(|m| m.get("primitives"))
        .and_then(|p| p.as_array())
        .ok_or_else(|| AppError::msg("mesh has no primitives"))?;

    for prim in primitives {
        let Some(pos_acc) = prim
            .get("attributes")
            .and_then(|a| a.get("POSITION"))
            .and_then(|v| v.as_u64())
        else {
            continue;
        };
        let acc = accessor_at(doc, pos_acc as usize)?;
        let (min, max) = accessor_min_max(acc)?;
        for corner in corners(min, max) {
            aabb.expand(mat4::transform_point(world, corner));
        }
    }
    Ok(())
}

/// Compute the corrected glTF: bake a translation onto the scene root so the
/// chosen BB point lands on the origin for `axis`, leaving the other axes as-is.
/// Returns the rewritten document and the resulting world AABB.
pub fn recenter(doc: &Value, axis: Axis, align: Align) -> AppResult<(Value, Aabb)> {
    let aabb = world_aabb(doc)?;
    let coord = aabb.align_coord(axis, align);

    // Translate by -coord on the target axis only.
    let mut delta: Vec3 = [0.0, 0.0, 0.0];
    delta[axis.index()] = -coord;

    let out = apply_root_translation(doc, delta)?;
    let new_aabb = world_aabb(&out)?;
    Ok((out, new_aabb))
}

/// Wrap the single scene root in a new node carrying `delta` as its translation,
/// composed above any existing root transform. Correct for skinned and static
/// assets alike (the whole hierarchy shifts together).
fn apply_root_translation(doc: &Value, delta: Vec3) -> AppResult<Value> {
    let mut out = doc.clone();

    let scene_idx = out.get("scene").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let old_root = scene_root_index(&out, scene_idx)?;

    // Append a new node that translates and parents the old root.
    let new_node = serde_json::json!({
        "name": "OriginCorrection",
        "translation": [delta[0], delta[1], delta[2]],
        "children": [old_root],
    });
    let nodes = out
        .get_mut("nodes")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| AppError::msg("no nodes array"))?;
    let new_index = nodes.len();
    nodes.push(new_node);

    // Point the scene at the new wrapper node.
    let scene_nodes = out
        .get_mut("scenes")
        .and_then(|v| v.as_array_mut())
        .and_then(|a| a.get_mut(scene_idx))
        .and_then(|s| s.get_mut("nodes"))
        .and_then(|n| n.as_array_mut())
        .ok_or_else(|| AppError::msg("scene nodes not writable"))?;
    scene_nodes.clear();
    scene_nodes.push(Value::from(new_index));

    Ok(out)
}

// --- helpers over the glTF JSON ---

fn active_scene(doc: &Value) -> AppResult<&Value> {
    let idx = doc.get("scene").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    doc.get("scenes")
        .and_then(|v| v.as_array())
        .and_then(|a| a.get(idx))
        .ok_or_else(|| AppError::msg("no active scene"))
}

fn scene_root_index(doc: &Value, scene_idx: usize) -> AppResult<usize> {
    let roots = doc
        .get("scenes")
        .and_then(|v| v.as_array())
        .and_then(|a| a.get(scene_idx))
        .and_then(|s| s.get("nodes"))
        .and_then(|n| n.as_array())
        .ok_or_else(|| AppError::msg("scene has no nodes"))?;
    if roots.len() != 1 {
        return Err(AppError::msg(format!(
            "expected exactly 1 scene root, found {}",
            roots.len()
        )));
    }
    Ok(roots[0].as_u64().ok_or_else(|| AppError::msg("bad root index"))? as usize)
}

fn node_at(doc: &Value, idx: usize) -> AppResult<&Value> {
    doc.get("nodes")
        .and_then(|v| v.as_array())
        .and_then(|a| a.get(idx))
        .ok_or_else(|| AppError::msg(format!("no node {idx}")))
}

fn accessor_at(doc: &Value, idx: usize) -> AppResult<&Value> {
    doc.get("accessors")
        .and_then(|v| v.as_array())
        .and_then(|a| a.get(idx))
        .ok_or_else(|| AppError::msg(format!("no accessor {idx}")))
}

/// A node's local matrix: explicit `matrix`, else composed from TRS.
fn node_local_matrix(node: &Value) -> AppResult<Mat4> {
    if let Some(m) = node.get("matrix").and_then(|v| v.as_array()) {
        if m.len() == 16 {
            let mut out = [0.0; 16];
            for (i, e) in m.iter().enumerate() {
                out[i] = e.as_f64().ok_or_else(|| AppError::msg("bad matrix element"))?;
            }
            return Ok(out);
        }
    }
    let t = read_vec3(node, "translation", [0.0, 0.0, 0.0])?;
    let r = read_quat(node)?;
    let s = read_vec3(node, "scale", [1.0, 1.0, 1.0])?;
    Ok(mat4::trs(t, r, s))
}

fn accessor_min_max(acc: &Value) -> AppResult<(Vec3, Vec3)> {
    let min = read_vec3_field(acc, "min")?;
    let max = read_vec3_field(acc, "max")?;
    Ok((min, max))
}

/// The 8 corners of an AABB given min/max.
fn corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [min[0], max[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [min[0], max[1], max[2]],
        [max[0], max[1], max[2]],
    ]
}

fn read_vec3(node: &Value, key: &str, default: Vec3) -> AppResult<Vec3> {
    match node.get(key) {
        None => Ok(default),
        Some(v) => read_vec3_value(v),
    }
}

fn read_vec3_field(v: &Value, key: &str) -> AppResult<Vec3> {
    read_vec3_value(
        v.get(key)
            .ok_or_else(|| AppError::msg(format!("accessor missing {key} (needs accessor bounds)")))?,
    )
}

fn read_vec3_value(v: &Value) -> AppResult<Vec3> {
    let a = v.as_array().ok_or_else(|| AppError::msg("expected array"))?;
    if a.len() < 3 {
        return Err(AppError::msg("expected vec3"));
    }
    Ok([
        a[0].as_f64().unwrap_or(0.0),
        a[1].as_f64().unwrap_or(0.0),
        a[2].as_f64().unwrap_or(0.0),
    ])
}

fn read_quat(node: &Value) -> AppResult<[f64; 4]> {
    match node.get("rotation") {
        None => Ok([0.0, 0.0, 0.0, 1.0]),
        Some(v) => {
            let a = v.as_array().ok_or_else(|| AppError::msg("bad rotation"))?;
            Ok([
                a[0].as_f64().unwrap_or(0.0),
                a[1].as_f64().unwrap_or(0.0),
                a[2].as_f64().unwrap_or(0.0),
                a[3].as_f64().unwrap_or(1.0),
            ])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A minimal glTF: scene -> root node -> mesh node, one primitive whose
    /// POSITION accessor bounds put the box at x∈[2,6], y∈[0,3], z∈[-5,-1].
    fn doc_offset() -> Value {
        json!({
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [
                { "name": "RootNode", "children": [1] },
                { "name": "Mesh", "mesh": 0 }
            ],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }],
            "accessors": [{
                "type": "VEC3", "componentType": 5126, "count": 8,
                "min": [2.0, 0.0, -5.0], "max": [6.0, 3.0, -1.0]
            }]
        })
    }

    #[test]
    fn computes_world_aabb() {
        let bb = world_aabb(&doc_offset()).unwrap();
        assert_eq!(bb.min, [2.0, 0.0, -5.0]);
        assert_eq!(bb.max, [6.0, 3.0, -1.0]);
    }

    #[test]
    fn recenter_y_min_grounds_base() {
        let (out, bb) = recenter(&doc_offset(), Axis::Y, Align::Min).unwrap();
        assert!((bb.min[1]).abs() < 1e-9, "base on origin, got {}", bb.min[1]);
        // Other axes unchanged.
        assert_eq!(bb.min[0], 2.0);
        assert_eq!(bb.max[2], -1.0);
        // A wrapper node was added and the scene points at it.
        assert_eq!(out["nodes"].as_array().unwrap().len(), 3);
        let root = out["scenes"][0]["nodes"][0].as_u64().unwrap();
        assert_eq!(out["nodes"][root as usize]["name"], "OriginCorrection");
    }

    #[test]
    fn recenter_x_center_zeroes_center() {
        let (_out, bb) = recenter(&doc_offset(), Axis::X, Align::Center).unwrap();
        let cx = 0.5 * (bb.min[0] + bb.max[0]);
        assert!(cx.abs() < 1e-9, "x center on origin, got {cx}");
    }

    #[test]
    fn recenter_z_max_lands_max_on_origin() {
        let (_out, bb) = recenter(&doc_offset(), Axis::Z, Align::Max).unwrap();
        assert!((bb.max[2]).abs() < 1e-9, "z max on origin, got {}", bb.max[2]);
    }

    #[test]
    fn corrections_compose_across_axes() {
        // Apply Y-min, then X-center on the result.
        let (out1, _) = recenter(&doc_offset(), Axis::Y, Align::Min).unwrap();
        let (_out2, bb) = recenter(&out1, Axis::X, Align::Center).unwrap();
        assert!((bb.min[1]).abs() < 1e-9, "y still grounded");
        let cx = 0.5 * (bb.min[0] + bb.max[0]);
        assert!(cx.abs() < 1e-9, "x now centered");
    }
}
