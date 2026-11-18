//! Lossless GLB assembly from separate-format glTF. The library's glTF are all
//! single-buffer, separate-format, no Draco/extensions, so we can build a binary
//! glTF by concatenating the existing `.bin` with each image's bytes into one
//! BIN chunk — preserving geometry bytes exactly (no re-encode) — then pointing
//! `images[].bufferView` at the appended image ranges and dropping all URIs.
//!
//! GLB layout (little-endian):
//!   header: magic "glTF" (0x46546C67), version=2, total length (u32)
//!   chunk 0: JSON  — length, type 0x4E4F534A ("JSON"), data padded with 0x20
//!   chunk 1: BIN   — length, type 0x004E4942 ("BIN\0"), data padded with 0x00

use serde_json::Value;

use crate::domain::gltf_parse;
use crate::error::{AppError, AppResult};

const GLB_MAGIC: u32 = 0x46546C67;
const GLB_VERSION: u32 = 2;
const CHUNK_JSON: u32 = 0x4E4F534A;
const CHUNK_BIN: u32 = 0x004E4942;

/// An external image to embed: its glTF `images[]` index, MIME type, and bytes.
pub struct EmbedImage {
    pub image_index: usize,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// Assemble a GLB. `gltf_doc` is the parsed separate-format document; `bin` is
/// its single buffer's bytes; `images` are the external images to embed (already
/// transcoded to a GLB-legal MIME — PNG/JPEG). Returns the GLB byte container.
pub fn assemble(gltf_doc: &Value, bin: &[u8], images: Vec<EmbedImage>) -> AppResult<Vec<u8>> {
    let mut doc = gltf_doc.clone();

    // One combined binary buffer: [original bin][img0][img1]... each 4-aligned.
    let mut binary: Vec<u8> = Vec::with_capacity(bin.len());
    binary.extend_from_slice(bin);

    let base_view_count = doc
        .get("bufferViews")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let mut new_views: Vec<Value> = Vec::new();
    for (i, img) in images.iter().enumerate() {
        pad_to_4(&mut binary, 0x00);
        let offset = binary.len();
        binary.extend_from_slice(&img.bytes);
        new_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": offset,
            "byteLength": img.bytes.len(),
        }));
        let view_index = base_view_count + i;
        gltf_parse::embed_buffer_view(&mut doc, "images", img.image_index, view_index, Some(&img.mime))?;
    }

    append_buffer_views(&mut doc, new_views)?;
    set_single_buffer(&mut doc, binary.len())?;

    let json = serde_json::to_vec(&doc)?;
    Ok(build_container(&json, &binary))
}

/// Append generated bufferViews to the document's `bufferViews` array.
fn append_buffer_views(doc: &mut Value, views: Vec<Value>) -> AppResult<()> {
    if views.is_empty() {
        return Ok(());
    }
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| AppError::msg("glTF root is not an object"))?;
    let arr = obj
        .entry("bufferViews")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| AppError::msg("bufferViews is not an array"))?;
    arr.extend(views);
    Ok(())
}

/// Replace `buffers` with a single embedded buffer of `byte_length` (no URI).
fn set_single_buffer(doc: &mut Value, byte_length: usize) -> AppResult<()> {
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| AppError::msg("glTF root is not an object"))?;
    obj.insert(
        "buffers".into(),
        Value::Array(vec![serde_json::json!({ "byteLength": byte_length })]),
    );
    Ok(())
}

/// Pad a byte vector up to a 4-byte boundary with `fill`.
fn pad_to_4(buf: &mut Vec<u8>, fill: u8) {
    while buf.len() % 4 != 0 {
        buf.push(fill);
    }
}

/// Build the GLB byte container from the JSON and BIN payloads.
fn build_container(json: &[u8], bin: &[u8]) -> Vec<u8> {
    let mut json_chunk = json.to_vec();
    pad_to_4(&mut json_chunk, 0x20); // spaces
    let mut bin_chunk = bin.to_vec();
    pad_to_4(&mut bin_chunk, 0x00);

    let total =
        12 + 8 + json_chunk.len() + 8 + bin_chunk.len();

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&GLB_MAGIC.to_le_bytes());
    out.extend_from_slice(&GLB_VERSION.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());

    out.extend_from_slice(&(json_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_JSON.to_le_bytes());
    out.extend_from_slice(&json_chunk);

    out.extend_from_slice(&(bin_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
    out.extend_from_slice(&bin_chunk);

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn assembles_valid_glb_header() {
        let doc = json!({
            "asset": { "version": "2.0" },
            "buffers": [{ "uri": "x.bin", "byteLength": 4 }],
            "bufferViews": [{ "buffer": 0, "byteOffset": 0, "byteLength": 4 }],
            "images": [{ "uri": "../textures/a.png" }]
        });
        let bin = vec![1u8, 2, 3, 4];
        let images = vec![EmbedImage {
            image_index: 0,
            mime: "image/png".into(),
            bytes: vec![9, 9, 9],
        }];

        let glb = assemble(&doc, &bin, images).unwrap();

        // Header
        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2);
        assert_eq!(
            u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize,
            glb.len()
        );
        // JSON chunk type
        assert_eq!(u32::from_le_bytes(glb[16..20].try_into().unwrap()), CHUNK_JSON);

        // Parse the embedded JSON and verify the image now points at a bufferView,
        // the buffer has no URI, and a new bufferView was appended.
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json_bytes = &glb[20..20 + json_len];
        let parsed: Value = serde_json::from_slice(json_bytes).unwrap();
        assert!(parsed["images"][0].get("uri").is_none());
        assert_eq!(parsed["images"][0]["bufferView"], 1);
        assert!(parsed["buffers"][0].get("uri").is_none());
        assert_eq!(parsed["bufferViews"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn total_length_is_4_aligned() {
        let doc = json!({
            "buffers": [{ "uri": "x.bin", "byteLength": 1 }],
            "images": []
        });
        let glb = assemble(&doc, &[7u8], vec![]).unwrap();
        assert_eq!(glb.len() % 4, 0);
    }
}
