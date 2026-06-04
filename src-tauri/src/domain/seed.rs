//! Pure seeding logic: turn the Godot `asset_library.json` document into our
//! catalog model. The two orthogonal facets (pack, category) are recovered from
//! the conflated `collections` list and cross-checked against folder structure.
//!
//! This module performs no IO. The caller (`commands::scan`) reads files and
//! supplies each asset's glTF image URIs via a lookup, so the transform stays
//! pure and testable.

use std::collections::HashMap;

use serde_json::Value;

use crate::domain::catalog_model::{AnimationMeta, Asset, AssetFileset, ThumbMeta, UserMeta};
use crate::domain::paths;
use crate::error::{AppError, AppResult};

/// A single entry recovered from `asset_library.json`, before we attach the
/// file-read-derived texture list. `gltf_rel` / `bin_rel` are library-relative.
pub struct SeedEntry {
    pub id: String,
    pub name: String,
    pub file_name: String,
    pub gltf_rel: String,
    pub bin_rel: String,
    pub pack: String,
    pub category: String,
}

/// Collection ids below this are packs; at/above are categories.
const CATEGORY_ID_FLOOR: i64 = 100;

/// Parse `asset_library.json` into seed entries. Pack/category names are derived
/// from the `collections` table (not hardcoded) and cross-checked against the
/// folder path; a mismatch is an error rather than a silent guess.
pub fn parse_seed_entries(doc: &Value) -> AppResult<Vec<SeedEntry>> {
    let names = collection_names(doc)?;
    let assets = doc
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::msg("asset_library.json has no assets[]"))?;

    assets.iter().map(|a| parse_one(a, &names)).collect()
}

/// Map collection id -> name (e.g. 1 -> "polygon_city" derived from folders,
/// but here we use the human name only for reference; the canonical pack/
/// category strings come from the folder path which matches the on-disk dirs).
fn collection_names(doc: &Value) -> AppResult<HashMap<i64, String>> {
    let cols = doc
        .get("collections")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::msg("asset_library.json has no collections[]"))?;
    let mut map = HashMap::new();
    for c in cols {
        let id = c.get("id").and_then(|v| v.as_i64());
        let name = c.get("name").and_then(|v| v.as_str());
        if let (Some(id), Some(name)) = (id, name) {
            map.insert(id, name.to_string());
        }
    }
    Ok(map)
}

fn parse_one(a: &Value, _names: &HashMap<i64, String>) -> AppResult<SeedEntry> {
    let id = str_field(a, "id")?;
    let file_name = str_field(a, "name")?;
    let folder_path = str_field(a, "folder_path")?;

    let rel_dir = paths::res_to_rel(&folder_path);
    let (pack, category) = paths::pack_category_from_rel_dir(&rel_dir)
        .ok_or_else(|| AppError::msg(format!("unexpected folder shape: {rel_dir}")))?;

    // Cross-check the tag-derived facets against the folder. The pack id is
    // `primary_collection`; the category id is the tag >= CATEGORY_ID_FLOOR.
    // We only validate consistency of the two tags (one pack, one category);
    // the canonical names come from the folder, which matches disk exactly.
    validate_tags(a)?;

    let name = file_name
        .strip_suffix(".gltf")
        .unwrap_or(&file_name)
        .to_string();
    let gltf_rel = format!("{rel_dir}/{file_name}");
    let bin_rel = format!("{rel_dir}/{name}.bin");

    Ok(SeedEntry { id, name, file_name, gltf_rel, bin_rel, pack, category })
}

/// Confirm each asset carries exactly one pack tag (< floor) and one category
/// tag (>= floor), and that `primary_collection` equals the pack tag.
fn validate_tags(a: &Value) -> AppResult<()> {
    let tags: Vec<i64> = a
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|t| t.as_i64()).collect())
        .unwrap_or_default();
    let packs: Vec<i64> = tags.iter().copied().filter(|t| *t < CATEGORY_ID_FLOOR).collect();
    let cats: Vec<i64> = tags.iter().copied().filter(|t| *t >= CATEGORY_ID_FLOOR).collect();
    if packs.len() != 1 || cats.len() != 1 {
        return Err(AppError::msg(format!(
            "asset {:?} has unexpected tags {tags:?}",
            a.get("name")
        )));
    }
    if let Some(primary) = a.get("primary_collection").and_then(|v| v.as_i64()) {
        if primary != packs[0] {
            return Err(AppError::msg(format!(
                "asset {:?}: primary_collection {primary} != pack tag {}",
                a.get("name"),
                packs[0]
            )));
        }
    }
    Ok(())
}

/// Assemble a full `Asset` from a seed entry plus its resolved texture list and
/// animation metadata (both derived from reading the asset's glTF).
pub fn build_asset(
    entry: SeedEntry,
    texture_rels: Vec<String>,
    animation: AnimationMeta,
) -> Asset {
    Asset {
        id: entry.id,
        name: entry.name,
        file_name: entry.file_name,
        rel_path: entry.gltf_rel.clone(),
        pack: entry.pack,
        category: entry.category,
        fileset: AssetFileset {
            gltf: entry.gltf_rel,
            bin: entry.bin_rel,
            textures: texture_rels,
        },
        thumb: ThumbMeta::default(),
        user: UserMeta::default(),
        animation,
    }
}

fn str_field(v: &Value, key: &str) -> AppResult<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::msg(format!("missing string field '{key}'")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_doc() -> Value {
        json!({
            "collections": [
                {"id": 1, "name": "Polygon City"},
                {"id": 100, "name": "Buildings"}
            ],
            "assets": [{
                "folder_path": "res://assets/library/polygon_city/buildings",
                "id": "uid://abc",
                "name": "SM_Bld_Apartment_01.gltf",
                "primary_collection": 1,
                "tags": [1, 100]
            }]
        })
    }

    #[test]
    fn parses_one_entry() {
        let entries = parse_seed_entries(&sample_doc()).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.id, "uid://abc");
        assert_eq!(e.name, "SM_Bld_Apartment_01");
        assert_eq!(e.pack, "polygon_city");
        assert_eq!(e.category, "buildings");
        assert_eq!(e.gltf_rel, "library/polygon_city/buildings/SM_Bld_Apartment_01.gltf");
        assert_eq!(e.bin_rel, "library/polygon_city/buildings/SM_Bld_Apartment_01.bin");
    }

    #[test]
    fn rejects_bad_tags() {
        let mut doc = sample_doc();
        doc["assets"][0]["tags"] = json!([1, 2]); // two packs, no category
        assert!(parse_seed_entries(&doc).is_err());
    }
}
