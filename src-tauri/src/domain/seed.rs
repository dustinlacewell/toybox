//! Pure seeding logic: turn Toybox's native `catalog.json` source document into
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
use std::path::Path;

use crate::domain::catalog_model::{
    AnimationMeta, Asset, AssetContent, AssetFileset, ThumbMeta, UserMeta,
};
use crate::domain::material::MaterialSet;
use crate::error::{AppError, AppResult};

/// A single entry recovered from `catalog.json`, before we attach the
/// file-read-derived texture/animation facts. All paths are library-relative.
/// `rel_path` is the `.gltf` for a model and the material's folder for a material.
pub struct SeedEntry {
    pub provenance: Option<crate::domain::provenance::ImportProvenance>,
    pub variants: Vec<SeedEntry>,
    pub id: String,
    pub name: String,
    pub file_name: String,
    pub rel_path: String,
    pub pack: String,
    pub category: String,
    pub content: SeedContent,
}

/// The kind-specific part of a seed entry. A material's map files are already
/// library-relative.
pub enum SeedContent {
    Audio { file: String },
    AnimationSource { gltf_rel: String, bin_rel: String },
    Model { gltf_rel: String, bin_rel: String },
    Material { material: MaterialSet },
}

/// Parse `catalog.json` into seed entries. Each asset names its pack, category,
/// and file directly; the library-relative paths are reconstructed as
/// `library/<pack>/<category>/<file>`. An entry's optional `kind` is `"model"`
/// (the default) or `"material"`.
pub fn parse_seed_entries(doc: &Value) -> AppResult<Vec<SeedEntry>> {
    let assets = doc
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::msg("catalog.json has no assets[]"))?;
    assets.iter().map(parse_entry).collect()
}

/// Category reserved for material sets: `library/<pack>/materials/` holds them,
/// so a model may not claim it.
const RESERVED_MATERIALS_CATEGORY: &str = "materials";

/// Parse one manifest entry (a `catalog.json` asset, or an importer's entry —
/// they share one shape). The single parser both paths go through.
pub fn parse_entry(a: &Value) -> AppResult<SeedEntry> {
    let mut entry = parse_single(a)?;
    if let Some(variants) = a.get("variants") {
        if matches!(entry.content, SeedContent::AnimationSource { .. }) {
            return Err(AppError::msg("animation sources contain clips, not level or size variants"));
        }
        let variants = variants.as_array().ok_or_else(|| AppError::msg("variants must be an array"))?;
        for variant in variants {
            if variant.get("variants").is_some() {
                return Err(AppError::msg("variants cannot contain nested variants"));
            }
            let mut parsed = parse_single(variant)?;
            if parsed.provenance.is_none(){parsed.provenance=entry.provenance.clone();}
            if std::mem::discriminant(&parsed.content) != std::mem::discriminant(&entry.content)
                || parsed.pack != entry.pack || parsed.category != entry.category
                || parsed.id == entry.id || entry.variants.iter().any(|v| v.id == parsed.id) {
                return Err(AppError::msg("variants must share the kind, pack and category and have unique ids"));
            }
            entry.variants.push(parsed);
        }
    }
    Ok(entry)
}

fn parse_single(a: &Value) -> AppResult<SeedEntry> {
    let mut entry=parse_single_content(a)?;
    entry.provenance=a.get("provenance").filter(|value|!value.is_null()).map(|value|serde_json::from_value::<crate::domain::provenance::ImportProvenance>(value.clone())).transpose()?.and_then(|value|value.normalized());
    Ok(entry)
}

fn parse_single_content(a: &Value) -> AppResult<SeedEntry> {
    let id = str_field(a, "id")?;
    let pack = str_field(a, "pack")?;
    let category = str_field(a, "category")?;
    match a.get("kind").map(|k| k.as_str()) {
        Some(Some("audio")) => {
            let file=str_field(a,"file")?;
            let mut entry=entry_from_parts(id,pack,category,file.clone());
            entry.name=Path::new(&file).file_stem().unwrap_or_default().to_string_lossy().to_string();
            entry.content=SeedContent::Audio{file:entry.rel_path.clone()};Ok(entry)
        }
        Some(Some("animation")) => {
            let file = str_field(a, "file")?;
            let mut entry = entry_from_parts(id, pack, category, file);
            if let SeedContent::Model { gltf_rel, bin_rel } = entry.content {
                entry.content = SeedContent::AnimationSource { gltf_rel, bin_rel };
            }
            Ok(entry)
        }
        None | Some(Some("model")) => {
            if category == RESERVED_MATERIALS_CATEGORY {
                return Err(AppError::msg(format!(
                    "asset {id}: category '{RESERVED_MATERIALS_CATEGORY}' is reserved for material sets"
                )));
            }
            let file = str_field(a, "file")?;
            Ok(entry_from_parts(id, pack, category, file))
        }
        Some(Some("material")) => {
            let name = str_field(a, "name")?;
            let decl: MaterialSet = serde_json::from_value(a.clone())?;
            material_entry_from_parts(id, pack, category, name, decl)
        }
        Some(_) => Err(AppError::msg(format!("asset {id}: unknown kind {}", a["kind"]))),
    }
}

/// Build a model `SeedEntry` from its four declared fields, reconstructing the
/// library-relative `gltf`/`bin` paths as `library/<pack>/<category>/<file>`.
/// Shared by `catalog.json` seeding (`parse_entry`) and plugin-driven import
/// (`commands::import`) so both produce identical entries.
pub fn entry_from_parts(id: String, pack: String, category: String, file: String) -> SeedEntry {
    let name = file.strip_suffix(".gltf").unwrap_or(&file).to_string();
    let rel_dir = format!("library/{pack}/{category}");
    let gltf_rel = format!("{rel_dir}/{file}");
    let bin_rel = format!("{rel_dir}/{name}.bin");

    SeedEntry {
        provenance: None,
        variants: Vec::new(),
        id,
        name,
        file_name: file,
        rel_path: gltf_rel.clone(),
        pack,
        category,
        content: SeedContent::Model { gltf_rel, bin_rel },
    }
}

/// Build a material `SeedEntry`. `decl` holds bare file names; each is rewritten
/// to `library/<pack>/materials/<name>/<file>`, and that folder is the entry's
/// `rel_path` (category is not part of the path). Fails if the set does not
/// validate.
pub fn material_entry_from_parts(
    id: String,
    pack: String,
    category: String,
    name: String,
    mut decl: MaterialSet,
) -> AppResult<SeedEntry> {
    let folder = format!("library/{pack}/materials/{name}");
    for map in &mut decl.maps {
        map.file = format!("{folder}/{}", map.file);
    }
    decl.validate()
        .map_err(|e| AppError::msg(format!("asset {id}: {e}")))?;

    Ok(SeedEntry {
        provenance: None,
        variants: Vec::new(),
        id,
        file_name: name.clone(),
        name,
        rel_path: folder,
        pack,
        category,
        content: SeedContent::Material { material: decl },
    })
}

/// Write `entries` into the manifest document `doc`: an entry whose `id` is
/// already in `assets[]` replaces that object in place, any other is appended.
/// Existing order and every other top-level field are left untouched.
pub fn upsert_entries(mut doc: Value, entries: Vec<Value>) -> AppResult<Value> {
    let assets = doc
        .get_mut("assets")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| AppError::msg("catalog.json has no assets[]"))?;
    for mut entry in entries {
        let id = str_field(&entry, "id")?;
        // Reimporting a formerly separate level/size must absorb its old row.
        let mut files = vec![physical_key(&entry)];
        if let Some(variants) = entry.get("variants").and_then(Value::as_array) {
            files.extend(variants.iter().map(physical_key));
        }
        files.retain(|key| !key.0.is_empty() && !key.2.is_empty());
        assets.retain(|old| old.get("id").and_then(Value::as_str) == Some(&id)
            || !files.contains(&physical_key(old)));
        match assets.iter().position(|a| a.get("id").and_then(|v| v.as_str()) == Some(&id)) {
            Some(i) => {
                if entry.get("provenance").is_none_or(Value::is_null) {
                    if let Some(provenance)=assets[i].get("provenance"){entry["provenance"]=provenance.clone();}
                }
                assets[i] = entry;
            },
            None => assets.push(entry),
        }
    }
    Ok(doc)
}

fn physical_key(entry: &Value) -> (String, String, String) {
    let string = |key: &str| entry.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    if entry.get("kind").and_then(Value::as_str) == Some("material") {
        (string("pack"), "material".into(), string("name"))
    } else {
        let category = if entry.get("kind").and_then(Value::as_str) == Some("animation") { format!("animation:{}", string("category")) } else { string("category") };
        (string("pack"), category, string("file"))
    }
}

/// Assemble a model `Asset` from a seed entry plus its resolved texture list and
/// animation metadata (both derived from reading the asset's glTF).
pub fn build_asset(
    entry: SeedEntry,
    texture_rels: Vec<String>,
    animation: AnimationMeta,
) -> AppResult<Asset> {
    let (head, content) = split(entry);
    match content {
        SeedContent::Model { gltf_rel, bin_rel } => Ok(head.into_asset(AssetContent::Model {
            fileset: AssetFileset { gltf: gltf_rel, bin: bin_rel, textures: texture_rels },
            animation,
        })),
        SeedContent::Material { .. } | SeedContent::AnimationSource { .. } | SeedContent::Audio { .. } => {
            Err(AppError::msg(format!("seed entry {} is not a model", head.id)))
        }
    }
}

/// Assemble a material `Asset` from a seed entry. Needs no file-read facts.
pub fn build_material_asset(entry: SeedEntry) -> AppResult<Asset> {
    let (head, content) = split(entry);
    match content {
        SeedContent::Material { material } => {
            Ok(head.into_asset(AssetContent::Material { material }))
        }
        SeedContent::Model { .. } | SeedContent::AnimationSource { .. } | SeedContent::Audio { .. } => {
            Err(AppError::msg(format!("seed entry {} is not a material", head.id)))
        }
    }
}

/// The fields every seed entry shares, separated from its kind-specific content.
pub fn build_audio_asset(entry:SeedEntry,audio:crate::domain::audio::AudioMeta)->Asset {
    let (head,_)=split(entry);head.into_asset(AssetContent::Audio{audio})
}

struct Head {
    provenance: Option<crate::domain::provenance::ImportProvenance>,
    id: String,
    name: String,
    file_name: String,
    rel_path: String,
    pack: String,
    category: String,
}

impl Head {
    fn into_asset(self, content: AssetContent) -> Asset {
        let tags=self.provenance.as_ref().map(|value|value.tags()).unwrap_or_default();
        Asset {
            rigs: Vec::new(),
            variants: Vec::new(),
            id: self.id,
            name: self.name,
            file_name: self.file_name,
            rel_path: self.rel_path,
            pack: self.pack,
            category: self.category,
            content,
            thumb: ThumbMeta::default(),
            user: UserMeta {provenance:self.provenance,tags,..UserMeta::default()},
        }
    }
}

fn split(entry: SeedEntry) -> (Head, SeedContent) {
    let head = Head {
        provenance: entry.provenance,
        id: entry.id,
        name: entry.name,
        file_name: entry.file_name,
        rel_path: entry.rel_path,
        pack: entry.pack,
        category: entry.category,
    };
    (head, entry.content)
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
    use crate::domain::material::MapChannel;
    use serde_json::json;

    #[test]
    fn parses_variant_filesets_and_rejects_mixed_kinds() {
        let mut doc = json!({ "id": "tree", "pack": "p", "category": "props", "file": "Tree_LOD0.gltf",
            "variants": [{ "id": "tree-low", "pack": "p", "category": "props", "file": "Tree_LOD2.gltf" }] });
        let parsed = parse_entry(&doc).unwrap();
        assert_eq!(parsed.variants.len(), 1);
        assert_eq!(parsed.variants[0].file_name, "Tree_LOD2.gltf");
        doc["variants"][0]["id"] = json!("tree");
        assert!(parse_entry(&doc).is_err(), "duplicate id is rejected");
        doc["variants"][0] = json!({ "id": "mat", "kind": "material", "pack": "p", "category": "props", "name": "Red", "maps": [{ "role": "albedo", "file": "red.png" }] });
        assert!(parse_entry(&doc).is_err(), "a material cannot be a model variant");
    }

    #[test]
    fn grouped_reimport_absorbs_previously_separate_variant_rows() {
        let original = json!({ "assets": [
            {"id":"old-full","pack":"p","category":"props","file":"Tree_LOD0.gltf"},
            {"id":"old-low","pack":"p","category":"props","file":"Tree_LOD2.gltf"},
            {"id":"other","pack":"p","category":"props","file":"Rock.gltf"}
        ] });
        let grouped = json!({ "id":"tree","pack":"p","category":"props","file":"Tree_LOD0.gltf", "variants":[
            {"id":"old-low","pack":"p","category":"props","file":"Tree_LOD2.gltf"}
        ] });
        let updated = upsert_entries(original, vec![grouped]).unwrap();
        let entries = updated["assets"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry["id"] == "other"));
        assert!(entries.iter().any(|entry| entry["id"] == "tree"));
    }

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
        assert_eq!(e.rel_path, "library/polygon_city/buildings/SM_Bld_Apartment_01.gltf");
        let SeedContent::Model { gltf_rel, bin_rel } = &e.content else {
            panic!("expected a model entry");
        };
        assert_eq!(gltf_rel, "library/polygon_city/buildings/SM_Bld_Apartment_01.gltf");
        assert_eq!(bin_rel, "library/polygon_city/buildings/SM_Bld_Apartment_01.bin");
    }

    #[test]
    fn entry_with_explicit_model_kind_is_a_model() {
        let mut doc = sample_doc();
        doc["assets"][0]["kind"] = json!("model");
        let entries = parse_seed_entries(&doc).unwrap();
        assert!(matches!(entries[0].content, SeedContent::Model { .. }));
    }

    fn material_doc() -> Value {
        json!({
            "assets": [{
                "id": "uid://m1", "kind": "material", "pack": "stone_pack",
                "category": "stone", "name": "CobbleWall",
                "maps": [
                    { "role": "albedo", "file": "CobbleWall_albedo.png" },
                    { "role": "roughness", "file": "CobbleWall_orm.png", "channel": "g" },
                    { "role": "ao", "file": "CobbleWall_orm.png", "channel": "r", "invert": false }
                ],
                "flipNormalGreen": false
            }]
        })
    }

    #[test]
    fn parses_a_material_entry() {
        let entries = parse_seed_entries(&material_doc()).unwrap();
        let e = &entries[0];
        let folder = "library/stone_pack/materials/CobbleWall";
        assert_eq!(e.id, "uid://m1");
        assert_eq!(e.name, "CobbleWall");
        assert_eq!(e.file_name, "CobbleWall");
        assert_eq!(e.category, "stone");
        assert_eq!(e.rel_path, folder);
        let SeedContent::Material { material } = &e.content else {
            panic!("expected a material entry");
        };
        assert_eq!(material.maps[0].file, format!("{folder}/CobbleWall_albedo.png"));
        assert_eq!(material.maps[1].file, format!("{folder}/CobbleWall_orm.png"));
        assert_eq!(material.maps[1].file, material.maps[2].file);
        assert_eq!(material.maps[1].channel, Some(MapChannel::G));
        assert_eq!(material.textures().len(), 2);
    }

    #[test]
    fn rejects_material_without_albedo() {
        let mut doc = material_doc();
        doc["assets"][0]["maps"].as_array_mut().unwrap().remove(0);
        assert!(parse_seed_entries(&doc).is_err());
    }

    #[test]
    fn rejects_unknown_kind() {
        let mut doc = sample_doc();
        doc["assets"][0]["kind"] = json!("sprite");
        assert!(parse_seed_entries(&doc).is_err());
    }

    #[test]
    fn builds_assets_of_each_kind() {
        let mut model = parse_seed_entries(&sample_doc()).unwrap();
        let a = build_asset(model.remove(0), vec![], AnimationMeta::default()).unwrap();
        assert!(a.fileset().is_ok());
        let mut mat = parse_seed_entries(&material_doc()).unwrap();
        let a = build_material_asset(mat.remove(0)).unwrap();
        assert!(a.fileset().is_err());
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

    #[test]
    fn rejects_model_in_reserved_materials_category() {
        let mut doc = sample_doc();
        doc["assets"][0]["category"] = json!("materials");
        assert!(parse_entry(&doc["assets"][0]).is_err());
    }

    #[test]
    fn material_entry_may_use_materials_category() {
        let mut doc = material_doc();
        doc["assets"][0]["category"] = json!("materials");
        assert!(parse_entry(&doc["assets"][0]).is_ok());
    }

    fn entry(id: &str, file: &str) -> Value {
        json!({ "id": id, "pack": "p", "category": "c", "file": file })
    }

    #[test]
    fn upsert_appends_a_new_entry() {
        let doc = json!({ "assets": [entry("a", "a.gltf")] });
        let out = upsert_entries(doc, vec![entry("b", "b.gltf")]).unwrap();
        assert_eq!(out["assets"], json!([entry("a", "a.gltf"), entry("b", "b.gltf")]));
    }

    #[test]
    fn upsert_replaces_same_id_in_place() {
        let doc = json!({ "assets": [entry("a", "a.gltf"), entry("b", "b.gltf")] });
        let out = upsert_entries(doc, vec![entry("a", "a2.gltf")]).unwrap();
        assert_eq!(out["assets"], json!([entry("a", "a2.gltf"), entry("b", "b.gltf")]));
    }

    #[test]
    fn upsert_leaves_top_level_fields_untouched() {
        let doc = json!({ "schemaVersion": 1, "note": "x", "assets": [] });
        let out = upsert_entries(doc, vec![entry("a", "a.gltf")]).unwrap();
        assert_eq!(out["schemaVersion"], json!(1));
        assert_eq!(out["note"], json!("x"));
    }

    #[test]
    fn upsert_errors_without_assets_or_id() {
        assert!(upsert_entries(json!({ "schemaVersion": 1 }), vec![entry("a", "a.gltf")]).is_err());
        assert!(upsert_entries(json!({ "assets": [] }), vec![json!({ "pack": "p" })]).is_err());
    }
}
