//! Versioned, lossless metadata transforms. No filename guesses or enrichment.
use serde_json::{json, Map, Value};
use std::collections::HashMap;

pub const LATEST_VERSION: u32 = 4;
pub const STEPS: &[(u32, &str)] = &[(1, "explicit-asset-kinds"), (2, "source-provenance"), (3, "typed-media-storage"), (4, "sqlite-store")];
const FIELDS: &[&str] = &["filesystemPath", "source", "publisher", "url", "license"];
pub type ProvenanceIndex = HashMap<String, Map<String, Value>>;

fn text(value: &Value) -> Option<&str> {
    value.as_str().filter(|value| !value.trim().is_empty())
}
fn insert_missing(target: &mut Map<String, Value>, field: &str, value: Option<&Value>) {
    if target.get(field).and_then(text).is_none() {
        if let Some(value) = value.filter(|value| text(value).is_some()) {
            target.insert(field.into(), value.clone());
        }
    }
}

pub fn provenance_index(doc: &Value, cache: bool) -> ProvenanceIndex {
    fn visit(entries: &[Value], cache: bool, found: &mut ProvenanceIndex) {
        for entry in entries {
            if let Some(id) = entry["id"].as_str() {
                let mut fields = (if cache {
                    entry.get("user").and_then(|user| user.get("provenance"))
                } else {
                    entry.get("provenance")
                })
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
                if cache {
                    for field in FIELDS {
                        insert_missing(
                            &mut fields,
                            field,
                            entry.get("provenance").and_then(|p| p.get(field)),
                        );
                    }
                    if let Some(user) = entry.get("user").and_then(Value::as_object) {
                        for field in FIELDS {
                            insert_missing(
                                &mut fields,
                                field,
                                user.get("provenance").and_then(|p| p.get(field)),
                            );
                        }
                        for field in FIELDS {
                            insert_missing(&mut fields, field, user.get(*field));
                        }
                        if let Some(tags) = user.get("tags").and_then(Value::as_array) {
                            for tag in tags.iter().filter_map(Value::as_str) {
                                if let Some((key, value)) = tag.split_once(':') {
                                    if ["source", "publisher", "license"].contains(&key)
                                        && !value.trim().is_empty()
                                    {
                                        insert_missing(
                                            &mut fields,
                                            key,
                                            Some(&json!(value.trim())),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                found.insert(id.into(), fields);
            }
            if let Some(variants) = entry["variants"].as_array() {
                visit(variants, cache, found);
            }
        }
    }
    let mut found = HashMap::new();
    if let Some(entries) = doc["assets"].as_array() {
        visit(entries, cache, &mut found);
    }
    found
}

/// Conflicting explicit values need a deliberate choice, never a guessed winner.
pub fn conflict(manifest: &ProvenanceIndex, cache: &ProvenanceIndex) -> Option<(String, String)> {
    for (id, fields) in manifest {
        if let Some(other) = cache.get(id) {
            for field in FIELDS {
                if let (Some(a), Some(b)) = (
                    fields.get(*field).and_then(text),
                    other.get(*field).and_then(text),
                ) {
                    if a != b {
                        return Some((id.clone(), field.to_string()));
                    }
                }
            }
        }
    }
    None
}

pub fn apply(
    doc: &mut Value,
    version: u32,
    cache: bool,
    packs: &HashMap<String, Value>,
    peers: &ProvenanceIndex,
) {
    let own = provenance_index(doc, cache);
    fn visit(
        entries: &mut [Value],
        version: u32,
        cache: bool,
        packs: &HashMap<String, Value>,
        own: &ProvenanceIndex,
        peers: &ProvenanceIndex,
        inherited_pack: Option<&str>,
        inherited_kind: &str,
    ) {
        for entry in entries {
            let pack = entry["pack"].as_str().or(inherited_pack).map(String::from);
            let kind = entry["kind"].as_str().unwrap_or(inherited_kind).to_string();
            if let Some(object) = entry.as_object_mut() {
                if version == 1 {
                    object.entry("kind").or_insert_with(|| json!(kind));
                }
                if version == 2 {
                    let id = object.get("id").and_then(Value::as_str).unwrap_or("");
                    let mut provenance = own.get(id).cloned().unwrap_or_default();
                    if let Some(peer) = peers.get(id) {
                        for field in FIELDS {
                            insert_missing(&mut provenance, field, peer.get(*field));
                        }
                    }
                    if let Some(slug) = pack.as_deref() {
                        if let Some(metadata) = packs.get(slug) {
                            insert_missing(&mut provenance, "source", metadata.get("name"));
                            for field in ["publisher", "url", "license"] {
                                insert_missing(&mut provenance, field, metadata.get(field));
                            }
                        }
                        // The old pack identifier directly corresponds to source grouping.
                        // Copy it verbatim if no explicit title exists; never humanize it.
                        insert_missing(&mut provenance, "source", Some(&json!(slug)));
                    }
                    if !provenance.is_empty() {
                        if cache {
                            let user = object.entry("user").or_insert_with(|| json!({}));
                            if let Some(user) = user.as_object_mut() {
                                let tags = user.entry("tags").or_insert_with(|| json!([]));
                                if let Some(tags) = tags.as_array_mut() {
                                    for field in ["source", "publisher", "license"] {
                                        if let Some(value) = provenance.get(field).and_then(text) {
                                            let tag = json!(format!("{field}: {value}"));
                                            if !tags.contains(&tag) {
                                                tags.push(tag);
                                            }
                                        }
                                    }
                                }
                                user.insert("provenance".into(), Value::Object(provenance));
                            }
                        } else {
                            object.insert("provenance".into(), Value::Object(provenance));
                        }
                    }
                }
                if let Some(variants) = object.get_mut("variants").and_then(Value::as_array_mut) {
                    visit(
                        variants,
                        version,
                        cache,
                        packs,
                        own,
                        peers,
                        pack.as_deref(),
                        &kind,
                    );
                }
            }
        }
    }
    if let Some(entries) = doc["assets"].as_array_mut() {
        visit(entries, version, cache, packs, &own, peers, None, "model");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn variants_inherit_their_declared_parent_kind() {
        let mut doc = json!({"assets":[{"id":"mat","kind":"material","variants":[{"id":"small"}]},{"id":"model","variants":[{"id":"low"}]}]});
        apply(&mut doc, 1, false, &HashMap::new(), &HashMap::new());
        assert_eq!(doc["assets"][0]["variants"][0]["kind"], "material");
        assert_eq!(doc["assets"][1]["kind"], "model");
        assert_eq!(doc["assets"][1]["variants"][0]["kind"], "model");
    }
    #[test]
    fn copies_corresponding_metadata_without_enrichment_or_overwriting_user_data() {
        let mut doc = json!({"schemaVersion":2,"assets":[{"id":"a","pack":"polygon_scifi","file":"Alien_CC0.gltf","user":{"favorite":true,"tags":["custom"],"collections":["Game"],"provenance":{"publisher":"User publisher"}},"variants":[{"id":"low"}]}]});
        let packs = HashMap::from([(
            "polygon_scifi".into(),
            json!({"name":"Sci-Fi Pack","publisher":"Recorded publisher","url":"https://example.test","color":"red"}),
        )]);
        apply(&mut doc, 1, true, &packs, &HashMap::new());
        apply(&mut doc, 2, true, &packs, &HashMap::new());
        let item = &doc["assets"][0];
        assert_eq!(item["id"], "a");
        assert_eq!(item["kind"], "model");
        assert_eq!(item["pack"], "polygon_scifi");
        assert_eq!(item["user"]["provenance"]["source"], "Sci-Fi Pack");
        assert_eq!(item["user"]["provenance"]["publisher"], "User publisher");
        assert!(item["user"]["provenance"].get("license").is_none());
        assert!(item["user"]["provenance"].get("filesystemPath").is_none());
        assert_eq!(item["user"]["collections"], json!(["Game"]));
        assert_eq!(item["user"]["favorite"], true);
        assert!(item["user"]["tags"]
            .as_array()
            .unwrap()
            .contains(&json!("custom")));
        assert_eq!(
            item["variants"][0]["user"]["provenance"]["source"],
            "Sci-Fi Pack"
        );
        let before = doc.clone();
        apply(&mut doc, 2, true, &packs, &HashMap::new());
        assert_eq!(doc, before);
    }
    #[test]
    fn unknown_provenance_fields_survive_and_conflicting_explicit_fields_are_reported() {
        let mut cache = json!({"assets":[{"id":"a","user":{"tags":[],"provenance":{"source":"User source","attribution":"Keep verbatim"}}}]});
        apply(&mut cache, 2, true, &HashMap::new(), &HashMap::new());
        assert_eq!(
            cache["assets"][0]["user"]["provenance"]["attribution"],
            "Keep verbatim"
        );
        let manifest = json!({"assets":[{"id":"a","provenance":{"source":"Other source"}}]});
        assert_eq!(
            conflict(
                &provenance_index(&manifest, false),
                &provenance_index(&cache, true)
            ),
            Some(("a".into(), "source".into()))
        );
    }
    #[test]
    fn preserves_exact_identifiers_and_copies_explicit_peer_fields() {
        let cache = json!({"assets":[{"id":"a","user":{"license":"CC BY 4.0","provenance":{"source":"Actual source"}}}]});
        let mut manifest = json!({"assets":[{"id":"a","pack":"keep_this_exact"},{"id":"b","pack":"do_NOT_title_case"}]});
        apply(
            &mut manifest,
            2,
            false,
            &HashMap::new(),
            &provenance_index(&cache, true),
        );
        assert_eq!(
            manifest["assets"][0]["provenance"],
            json!({"source":"Actual source","license":"CC BY 4.0"})
        );
        assert_eq!(
            manifest["assets"][1]["provenance"]["source"],
            "do_NOT_title_case"
        );
    }
}
