//! Library tags: descriptive keywords that belong to an asset and live in the
//! library manifest, so they travel with the asset to a server and to every
//! client. They are not the personal tags in `Asset.user.tags`.
//!
//! A tag is normalised on the way in: whitespace is trimmed and collapsed,
//! letters are lower-cased, empties and duplicates are dropped (the first
//! spelling keeps its place). A tag with a colon is dropped, so it can never be
//! mistaken for the `source: ...` / `publisher: ...` / `license: ...` tags that
//! are derived from provenance. Too many tags, or too long a tag, is cut
//! rather than refused.

use serde_json::Value;

/// Tags kept per asset; the rest are dropped.
pub const MAX_TAGS: usize = 64;
/// Characters kept per tag; a longer tag is cut to this length.
pub const MAX_TAG_CHARS: usize = 48;

/// Normalise one tag, or `None` when nothing usable is left.
pub fn normalize_tag(raw: &str) -> Option<String> {
    if raw.contains(':') {
        return None;
    }
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned: String = collapsed.chars().filter(|c| !c.is_control()).collect();
    let lowered = cleaned.to_lowercase();
    let cut: String = lowered.chars().take(MAX_TAG_CHARS).collect();
    let cut = cut.trim();
    if cut.is_empty() { None } else { Some(cut.to_string()) }
}

/// Normalise a list of tags: see the module notes. Order is kept.
pub fn normalize_tags<I, S>(raw: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: Vec<String> = Vec::new();
    for tag in raw {
        if out.len() >= MAX_TAGS {
            break;
        }
        if let Some(tag) = normalize_tag(tag.as_ref()) {
            if !out.contains(&tag) {
                out.push(tag);
            }
        }
    }
    out
}

/// Whether `tag` is one of the `source: ...` / `publisher: ...` / `license: ...`
/// tags derived from provenance (they live in `user.tags` and are not tags to
/// edit or migrate).
pub fn is_provenance_tag(tag: &str) -> bool {
    let lower = tag.trim_start().to_lowercase();
    ["source:", "publisher:", "license:"].iter().any(|prefix| lower.starts_with(prefix))
}

/// The normalised `tags` of a manifest entry. Absent, or not a list, is none;
/// items that are not strings are ignored.
pub fn from_entry(entry: &Value) -> Vec<String> {
    match entry.get("tags").and_then(Value::as_array) {
        Some(list) => normalize_tags(list.iter().filter_map(Value::as_str)),
        None => Vec::new(),
    }
}

/// Rewrite `tags` of a manifest entry (and of each of its variants) in
/// normalised form; an entry left without any has no `tags` field at all.
pub fn normalize_entry(entry: &mut Value) {
    if let Some(object) = entry.as_object_mut() {
        if object.contains_key("tags") {
            let tags = from_entry(&Value::Object(object.clone()));
            if tags.is_empty() {
                object.shift_remove("tags");
            } else {
                object.insert("tags".into(), Value::from(tags));
            }
        }
    }
    if let Some(variants) = entry.get_mut("variants").and_then(Value::as_array_mut) {
        for variant in variants {
            if let Some(object) = variant.as_object_mut() {
                if object.contains_key("tags") {
                    let tags = from_entry(&Value::Object(object.clone()));
                    if tags.is_empty() {
                        object.shift_remove("tags");
                    } else {
                        object.insert("tags".into(), Value::from(tags));
                    }
                }
            }
        }
    }
}

/// A copy of `entry` without `tags` (and without the `tags` of its variants):
/// what identifies an entry for storage. Tags are descriptive metadata that
/// can change at any time, so they must not move an asset's files.
pub fn without_tags(entry: &Value) -> Value {
    let mut copy = entry.clone();
    if let Some(object) = copy.as_object_mut() {
        object.shift_remove("tags");
    }
    if let Some(variants) = copy.get_mut("variants").and_then(Value::as_array_mut) {
        for variant in variants {
            if let Some(object) = variant.as_object_mut() {
                object.shift_remove("tags");
            }
        }
    }
    copy
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tags_are_trimmed_collapsed_lowered_deduplicated_and_keep_their_order() {
        let out = normalize_tags(["  Foot   Step ", "footstep", "FOOTSTEP", "", "   ", "Carpet", "foot step", "\tLoop\n"]);
        assert_eq!(out, vec!["foot step", "footstep", "carpet", "loop"]);
    }

    #[test]
    fn a_tag_with_a_colon_is_dropped_so_it_cannot_pose_as_a_provenance_tag() {
        assert_eq!(normalize_tags(["source: Synty", "license:CC0", "ui", "a:b"]), vec!["ui"]);
        assert_eq!(normalize_tag("publisher: x"), None);
    }

    #[test]
    fn excess_is_dropped_without_an_error() {
        let many: Vec<String> = (0..100).map(|n| format!("tag{n}")).collect();
        let out = normalize_tags(&many);
        assert_eq!(out.len(), MAX_TAGS);
        assert_eq!(out[0], "tag0");
        assert_eq!(out[MAX_TAGS - 1], format!("tag{}", MAX_TAGS - 1));
        let long = "x".repeat(200);
        let out = normalize_tags([long.as_str()]);
        assert_eq!(out[0].chars().count(), MAX_TAG_CHARS);
        // Cutting can make two long tags equal; they collapse to one.
        let twin = format!("{}{}", "y".repeat(MAX_TAG_CHARS), "z");
        assert_eq!(normalize_tags([twin.as_str(), &"y".repeat(MAX_TAG_CHARS)]).len(), 1);
        // Multi-byte characters are cut on character boundaries.
        let wide = "é".repeat(60);
        assert_eq!(normalize_tags([wide.as_str()])[0].chars().count(), MAX_TAG_CHARS);
    }

    #[test]
    fn entries_without_usable_tags_have_none() {
        assert!(from_entry(&json!({"id": "a"})).is_empty());
        assert!(from_entry(&json!({"tags": "coin"})).is_empty());
        assert_eq!(from_entry(&json!({"tags": ["Coin", 3, null, "coin", "ui"]})), vec!["coin", "ui"]);
        let mut entry = json!({"tags": [" ", "a:b"], "variants": [{"id": "v", "tags": [" Big "]}]});
        normalize_entry(&mut entry);
        assert!(entry.get("tags").is_none());
        assert_eq!(entry["variants"][0]["tags"], json!(["big"]));
        assert_eq!(without_tags(&entry)["variants"][0], json!({"id": "v"}));
    }
}
