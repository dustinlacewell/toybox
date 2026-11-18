//! FBX shader-mask data is not necessarily a glTF base-colour multiplier.
//! Repair only the observed opaque, textured, non-PBR FBX signature with a
//! majority of completely zero RGBA values. Authored RGB/opaque-black colour
//! attributes and PBR materials are deliberately retained.
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorRepair { pub mesh: usize, pub primitive: usize, pub zero_vertices: usize, pub vertices: usize }

pub fn may_need_repair(doc: &Value) -> bool {
    doc["materials"].as_array().into_iter().flatten().any(|material|
        material["extras"]["fromFBX"]["isTruePBR"].as_bool() == Some(false))
        && doc["meshes"].as_array().into_iter().flatten().any(|mesh|
            mesh["primitives"].as_array().into_iter().flatten().any(|primitive|
                primitive["attributes"]["COLOR_0"].is_number()))
}

pub fn repair(doc: &mut Value, buffers: &[Vec<u8>]) -> Vec<ColorRepair> {
    let mut changes = Vec::new();
    for (mesh_index, mesh) in doc["meshes"].as_array().into_iter().flatten().enumerate() {
        for (primitive_index, primitive) in mesh["primitives"].as_array().into_iter().flatten().enumerate() {
            let Some(material) = primitive["material"].as_u64().and_then(|index| doc["materials"].get(index as usize)) else { continue };
            if material["extras"]["fromFBX"]["isTruePBR"].as_bool() != Some(false)
                || material["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64().is_none()
                || material["alphaMode"].as_str().is_some_and(|mode| mode != "OPAQUE") { continue; }
            let Some(accessor) = primitive["attributes"]["COLOR_0"].as_u64().and_then(|index| doc["accessors"].get(index as usize)) else { continue };
            let Some((zeros, count)) = zero_rgba_count(doc, accessor, buffers) else { continue };
            if count > 0 && zeros > count / 2 { changes.push(ColorRepair { mesh: mesh_index, primitive: primitive_index, zero_vertices: zeros, vertices: count }); }
        }
    }
    for change in &changes {
        if let Some(attributes) = doc["meshes"][change.mesh]["primitives"][change.primitive]["attributes"].as_object_mut() { attributes.remove("COLOR_0"); }
    }
    changes
}

fn zero_rgba_count(doc: &Value, accessor: &Value, buffers: &[Vec<u8>]) -> Option<(usize, usize)> {
    if accessor["type"].as_str()? != "VEC4" || accessor.get("sparse").is_some() { return None; }
    let component = accessor["componentType"].as_u64()?;
    let width = match component { 5121 => 1, 5123 => 2, 5126 => 4, _ => return None };
    let count = usize::try_from(accessor["count"].as_u64()?).ok()?;
    let view = doc["bufferViews"].get(accessor["bufferView"].as_u64()? as usize)?;
    let data = buffers.get(view["buffer"].as_u64()? as usize)?;
    let view_offset = usize::try_from(view["byteOffset"].as_u64().unwrap_or(0)).ok()?;
    let accessor_offset = usize::try_from(accessor["byteOffset"].as_u64().unwrap_or(0)).ok()?;
    let offset = view_offset.checked_add(accessor_offset)?;
    let stride = usize::try_from(view["byteStride"].as_u64().unwrap_or(width * 4)).ok()?;
    let size = width as usize * 4;
    if stride < size { return None; }
    let view_end = view_offset.checked_add(usize::try_from(view["byteLength"].as_u64()?).ok()?)?;
    let mut zeros = 0;
    for index in 0..count {
        let start = offset.checked_add(index.checked_mul(stride)?)?;
        let end = start.checked_add(size)?;
        if end > view_end { return None; }
        let bytes = data.get(start..end)?;
        let zero = if component == 5126 {
            bytes.chunks_exact(4).all(|value| f32::from_le_bytes(value.try_into().unwrap()).abs() < 1e-7)
        } else { bytes.iter().all(|byte| *byte == 0) };
        if zero { zeros += 1; }
    }
    Some((zeros, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (Value, Vec<Vec<u8>>) {
        let mut data = vec![0u8; 4]; // Offset + interleaved stride are intentional.
        for color in [[0.0f32; 4], [0.0; 4], [1.0; 4]] { for value in color { data.extend(value.to_le_bytes()); } data.extend([123u8; 4]); }
        (json!({"materials":[{"extras":{"fromFBX":{"isTruePBR":false}},"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}],"meshes":[{"primitives":[{"material":0,"attributes":{"POSITION":1,"COLOR_0":0}}]}],"bufferViews":[{"buffer":0,"byteOffset":4,"byteLength":60,"byteStride":20}],"accessors":[{"type":"VEC4","componentType":5126,"count":3,"bufferView":0}]}), vec![data])
    }
    #[test]
    fn removes_only_known_legacy_mask_preserving_geometry_and_material_references() {
        let (mut doc, buffers) = fixture(); let repairs = repair(&mut doc, &buffers);
        assert_eq!(repairs.len(), 1); assert_eq!(repairs[0].zero_vertices, 2);
        assert!(doc["meshes"][0]["primitives"][0]["attributes"].get("COLOR_0").is_none());
        assert_eq!(doc["meshes"][0]["primitives"][0]["attributes"]["POSITION"], 1);
        assert_eq!(doc["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"], 0);
        assert!(repair(&mut doc, &buffers).is_empty());
    }
    #[test]
    fn retains_authored_pbr_and_transparent_vertex_colours_and_truncated_buffers() {
        let (doc, buffers) = fixture();
        let mut pbr = doc.clone(); pbr["materials"][0]["extras"]["fromFBX"]["isTruePBR"] = json!(true); assert!(repair(&mut pbr, &buffers).is_empty());
        let mut transparent = doc.clone(); transparent["materials"][0]["alphaMode"] = json!("BLEND"); assert!(repair(&mut transparent, &buffers).is_empty());
        let mut untextured = doc.clone(); untextured["materials"][0]["pbrMetallicRoughness"] = json!({}); assert!(repair(&mut untextured, &buffers).is_empty());
        let mut truncated = doc.clone(); assert!(repair(&mut truncated, &[vec![0; 2]]).is_empty());
        let mut opaque_black = buffers.clone(); for index in 0..3 { opaque_black[0][4 + index * 20 + 12..4 + index * 20 + 16].copy_from_slice(&1.0f32.to_le_bytes()); }
        let mut unchanged = doc; assert!(repair(&mut unchanged, &opaque_black).is_empty());
    }
}
