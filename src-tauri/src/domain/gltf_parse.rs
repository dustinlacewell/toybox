//! Minimal, lossless glTF JSON access. We parse into `serde_json::Value` to
//! preserve every field byte-for-byte on round-trip (the exporters re-emit the
//! same document with only buffer/image URIs rewritten). Typed accessors here
//! pull out just the fields the catalog and exporters need.

use crate::error::{AppError, AppResult};
use serde_json::Value;

/// A reference to an external resource inside a glTF, with the array index it
/// occupies (needed when rewriting `buffers[i].uri` / `images[i].uri`).
#[derive(Debug, Clone)]
pub struct UriRef {
    pub index: usize,
    pub uri: String,
}

/// Parse glTF JSON text into a mutable document.
pub fn parse(text: &str) -> AppResult<Value> {
    Ok(serde_json::from_str(text)?)
}

/// The `uri` of every `buffers[i]` that has one (separate-format buffers).
pub fn buffer_uris(doc: &Value) -> Vec<UriRef> {
    array_uris(doc, "buffers")
}

/// The `uri` of every `images[i]` that has one.
pub fn image_uris(doc: &Value) -> Vec<UriRef> {
    array_uris(doc, "images")
}

/// Names of `animations[]` that actually carry channels. FBX→glTF converters
/// often emit an empty default take ("Take 001" with 0 channels); those are
/// filtered out so an asset is only "animated" if it has real clip data.
pub fn animation_clip_names(doc: &Value) -> Vec<String> {
    let Some(anims) = doc.get("animations").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    anims
        .iter()
        .filter(|a| {
            a.get("channels")
                .and_then(|c| c.as_array())
                .map(|c| !c.is_empty())
                .unwrap_or(false)
        })
        .enumerate()
        .map(|(i, a)| {
            a.get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("Clip {i}"))
        })
        .collect()
}

/// The MIME type declared on `images[i]`, if any.
pub fn image_mime(doc: &Value, index: usize) -> Option<String> {
    doc.get("images")?
        .as_array()?
        .get(index)?
        .get("mimeType")?
        .as_str()
        .map(|s| s.to_string())
}

fn array_uris(doc: &Value, key: &str) -> Vec<UriRef> {
    let Some(arr) = doc.get(key).and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .enumerate()
        .filter_map(|(index, item)| {
            item.get("uri")
                .and_then(|u| u.as_str())
                .map(|uri| UriRef { index, uri: uri.to_string() })
        })
        .collect()
}

/// Set `<key>[index].uri` to `new_uri`.
pub fn set_uri(doc: &mut Value, key: &str, index: usize, new_uri: &str) -> AppResult<()> {
    let item = doc
        .get_mut(key)
        .and_then(|v| v.as_array_mut())
        .and_then(|a| a.get_mut(index))
        .ok_or_else(|| AppError::msg(format!("glTF has no {key}[{index}]")))?;
    item["uri"] = Value::String(new_uri.to_string());
    Ok(())
}

/// Remove `<key>[index].uri` and set `<key>[index].bufferView = bv` (used when
/// embedding an external resource into a GLB's binary chunk).
pub fn embed_buffer_view(
    doc: &mut Value,
    key: &str,
    index: usize,
    buffer_view: usize,
    mime: Option<&str>,
) -> AppResult<()> {
    let item = doc
        .get_mut(key)
        .and_then(|v| v.as_array_mut())
        .and_then(|a| a.get_mut(index))
        .ok_or_else(|| AppError::msg(format!("glTF has no {key}[{index}]")))?;
    let obj = item
        .as_object_mut()
        .ok_or_else(|| AppError::msg(format!("{key}[{index}] is not an object")))?;
    obj.remove("uri");
    obj.insert("bufferView".into(), Value::from(buffer_view));
    if let Some(m) = mime {
        obj.insert("mimeType".into(), Value::String(m.to_string()));
    }
    Ok(())
}
