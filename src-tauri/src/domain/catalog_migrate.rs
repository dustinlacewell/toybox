//! Upgrades an older cached catalog document so it parses under the current
//! model. Pure JSON-to-JSON; no IO.
//!
//! The cache holds user favorites and tags, so an old schema must still load.
//! `schemaVersion` is left alone: the loader's version filter still rejects the
//! old cache, and the frontend rescans.

use serde_json::{json, Value};

/// Give every asset without a `kind` the `model` kind (v2 had models only).
pub fn upgrade(mut doc: Value) -> Value {
    if let Some(assets) = doc.get_mut("assets").and_then(Value::as_array_mut) {
        for asset in assets.iter_mut().filter_map(Value::as_object_mut) {
            asset.entry("kind").or_insert_with(|| json!("model"));
        }
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::catalog_model::Catalog;

    fn v2_doc() -> Value {
        json!({
            "schemaVersion": 2,
            "libraryRoot": "D:/lib",
            "assets": [{
                "id": "uid://a",
                "name": "N",
                "fileName": "N.gltf",
                "relPath": "library/p/c/N.gltf",
                "pack": "p",
                "category": "c",
                "fileset": { "gltf": "g", "bin": "b", "textures": ["t"] },
                "animation": { "clipCount": 1, "clipNames": ["Run"] },
                "thumb": { "state": "ready" },
                "user": { "favorite": true, "tags": ["x"] }
            }]
        })
    }

    #[test]
    fn v2_doc_upgrades_and_parses() {
        let catalog: Catalog = serde_json::from_value(upgrade(v2_doc())).unwrap();
        assert_eq!(catalog.schema_version, 2);
        let asset = &catalog.assets[0];
        assert_eq!(asset.fileset().unwrap().gltf, "g");
        assert_eq!(asset.animation().unwrap().clip_count, 1);
        assert!(asset.user.favorite);
    }

    #[test]
    fn existing_kind_is_left_alone() {
        let doc = json!({ "assets": [{ "kind": "material", "id": "m" }] });
        assert_eq!(upgrade(doc.clone()), doc);
    }

    #[test]
    fn doc_without_assets_is_unchanged() {
        let doc = json!({ "schemaVersion": 2, "libraryRoot": "D:/lib" });
        assert_eq!(upgrade(doc.clone()), doc);
    }
}
