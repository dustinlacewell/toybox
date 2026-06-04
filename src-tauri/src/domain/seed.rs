//! Pure seeding logic: turn Capital's native `catalog.json` source document into
//! our catalog model. The source is a flat asset list — each entry already names
//! its two facets (`pack`, `category`) explicitly, so seeding is a direct
//! projection with no id/tag archaeology.
//!
//! This module performs no IO. The caller (`commands::scan`) reads files and
//! supplies each asset's glTF-derived texture/animation facts, so the transform
//! stays pure and testable.
//!
//! Source schema (`schemaVersion: 1`):
//! ```json
//! { "schemaVersion": 1,
//!   "assets": [
//!     { "id": "uid://p6p3bf5qgcvb", "pack": "polygon_city",
//!       "category": "buildings", "file": "SM_Bld_Apartment_01.gltf" } ] }
//! ```

use serde_json::Value;

use crate::domain::catalog_model::{AnimationMeta, Asset, AssetFileset, ThumbMeta, UserMeta};
use crate::error::{AppError, AppResult};

/// A single entry recovered from `catalog.json`, before we attach the
/// file-read-derived texture/animation facts. `gltf_rel` / `bin_rel` are
/// library-relative.
pub struct SeedEntry {
    pub id: String,
    pub name: String,
    pub file_name: String,
    pub gltf_rel: String,
    pub bin_rel: String,
    pub pack: String,
    pub category: String,
}

/// Parse `catalog.json` into seed entries. Each asset names its pack, category,
/// and file directly; the library-relative paths are reconstructed as
/// `library/<pack>/<category>/<file>`.
pub fn parse_seed_entries(doc: &Value) -> AppResult<Vec<SeedEntry>> {
    let assets = doc
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::msg("catalog.json has no assets[]"))?;
    assets.iter().map(parse_one).collect()
}

fn parse_one(a: &Value) -> AppResult<SeedEntry> {
    let id = str_field(a, "id")?;
    let pack = str_field(a, "pack")?;
    let category = str_field(a, "category")?;
    let file_name = str_field(a, "file")?;

    let name = file_name
        .strip_suffix(".gltf")
        .unwrap_or(&file_name)
        .to_string();
    let rel_dir = format!("library/{pack}/{category}");
    let gltf_rel = format!("{rel_dir}/{file_name}");
    let bin_rel = format!("{rel_dir}/{name}.bin");

    Ok(SeedEntry { id, name, file_name, gltf_rel, bin_rel, pack, category })
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
            "schemaVersion": 1,
            "assets": [{
                "id": "uid://abc",
                "pack": "polygon_city",
                "category": "buildings",
                "file": "SM_Bld_Apartment_01.gltf"
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
    fn rejects_missing_field() {
        let mut doc = sample_doc();
        doc["assets"][0].as_object_mut().unwrap().remove("category");
        assert!(parse_seed_entries(&doc).is_err());
    }

    #[test]
    fn rejects_missing_assets_array() {
        assert!(parse_seed_entries(&json!({ "schemaVersion": 1 })).is_err());
    }
}
