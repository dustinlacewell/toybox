//! Build / merge the asset_placer addon's `asset_library.json` from Capital's
//! own facets. This is a **one-way publish** format: Capital never re-seeds its
//! catalog from it (see `seed.rs` for the inbound source). Pure data — no IO.
//!
//! The addon's schema (`version: 3`) is three arrays — `collections`, `assets`,
//! `folders`. A collection is a tag with an integer id; an asset carries
//! `tags: int[]` plus a `primary_collection`. We derive collections from four
//! Capital facets and band their ids so the kinds stay visually separable and
//! never collide on allocation:
//!
//!   pack      -> 1..=99      (primary collection of each asset)
//!   category  -> 100..=999
//!   favorites -> 1000        (single fixed slot)
//!   user-tag  -> 1001..
//!
//! Merging into an existing library is idempotent: a collection whose `name`
//! already exists is reused by its id (never duplicated); an asset already
//! present (keyed `folder_path|name`) is left untouched. Re-exporting, or
//! exporting into a project that already holds other packs, is therefore safe.

use std::collections::HashMap;

use serde_json::{json, Value};

/// One asset to publish: the Capital facets that become tags, plus the
/// `res://` path the file was written to and the basename the addon stores as
/// `name`. `folder_path` is the parent dir of `res_path` (addon convention).
pub struct PlacerAsset {
    pub pack: String,
    pub category: String,
    pub favorite: bool,
    pub tags: Vec<String>,
    /// Full `res://…/Asset.glb` path of the written file.
    pub res_path: String,
    /// Basename incl. extension, e.g. `Asset.glb`.
    pub name: String,
}

/// Id bands. Allocation within a band takes `max(existing in band) + 1`, so it
/// is append-only and stable across runs.
const PACK_FLOOR: i64 = 1;
const CATEGORY_FLOOR: i64 = 100;
const FAVORITES_ID: i64 = 1000;
const USER_TAG_FLOOR: i64 = 1001;

const FAVORITES_NAME: &str = "Favorites";

/// A 16-entry distinct palette, assigned by allocation order. Matches the
/// generator tooling's palette so colors stay familiar across surfaces.
const PALETTE: [&str; 16] = [
    "#4a90e2", "#7ed321", "#f5a623", "#bd10e0", "#50e3c2", "#e94e77", "#9013fe", "#417505",
    "#f8e71c", "#d0021b", "#8b572a", "#5b8def", "#3da97c", "#c45ad9", "#9b9b9b", "#4a4a4a",
];

/// Merge `assets` into `doc` (an existing parsed `asset_library.json`, or
/// `empty_library()`), returning the updated document. Pure: the caller writes
/// it. Folders[] gains nothing here — the command registers the recursive
/// root folder separately since it depends on the chosen layout base.
pub fn merge(doc: &Value, assets: &[PlacerAsset]) -> Value {
    let mut collections = Collections::from_doc(doc);
    let existing_keys = existing_asset_keys(doc);

    let mut out_assets: Vec<Value> = doc
        .get("assets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for a in assets {
        let folder_path = parent_of(&a.res_path);
        if existing_keys.contains(&asset_key(&folder_path, &a.name)) {
            continue; // idempotent: already published
        }
        let pack_id = collections.ensure(Facet::Pack, &a.pack);
        let mut tags = vec![pack_id, collections.ensure(Facet::Category, &a.category)];
        if a.favorite {
            tags.push(collections.ensure(Facet::Favorites, FAVORITES_NAME));
        }
        for tag in &a.tags {
            tags.push(collections.ensure(Facet::UserTag, tag));
        }
        out_assets.push(json!({
            "id": a.res_path,
            "name": a.name,
            "tags": tags,
            "folder_path": folder_path,
            "primary_collection": pack_id,
        }));
    }

    json!({
        "version": 3,
        "collections": collections.into_values(),
        "assets": out_assets,
        "folders": doc.get("folders").cloned().unwrap_or_else(|| json!([])),
    })
}

/// Register a recursive folder root so the addon's own Sync feature discovers
/// assets added under `base_res` after this export. Idempotent by path.
pub fn ensure_folder(doc: &mut Value, base_res: &str) {
    let folders = doc
        .get_mut("folders")
        .and_then(Value::as_array_mut)
        .expect("merge() always emits a folders array");
    let present = folders
        .iter()
        .any(|f| f.get("path").and_then(Value::as_str) == Some(base_res));
    if !present {
        folders.push(json!({
            "path": base_res,
            "include_subfolders": true,
            "rules": [],
        }));
    }
}

/// An empty `version: 3` library, the starting point when no file exists yet.
pub fn empty_library() -> Value {
    json!({ "version": 3, "collections": [], "assets": [], "folders": [] })
}

// --- collection bookkeeping ------------------------------------------------

#[derive(Clone, Copy)]
enum Facet {
    Pack,
    Category,
    Favorites,
    UserTag,
}

impl Facet {
    fn floor(self) -> i64 {
        match self {
            Facet::Pack => PACK_FLOOR,
            Facet::Category => CATEGORY_FLOOR,
            Facet::Favorites => FAVORITES_ID,
            Facet::UserTag => USER_TAG_FLOOR,
        }
    }
    /// Upper bound (exclusive) used to scan existing ids in this band.
    fn ceiling(self) -> i64 {
        match self {
            Facet::Pack => CATEGORY_FLOOR,
            Facet::Category => FAVORITES_ID,
            Facet::Favorites => USER_TAG_FLOOR,
            Facet::UserTag => i64::MAX,
        }
    }
}

/// Existing + newly-minted collections, indexed by name for reuse and ordered
/// for stable output.
struct Collections {
    by_name: HashMap<String, i64>,
    order: Vec<(i64, String, String)>, // (id, name, color)
    palette_idx: usize,
}

impl Collections {
    fn from_doc(doc: &Value) -> Self {
        let mut by_name = HashMap::new();
        let mut order = Vec::new();
        if let Some(cols) = doc.get("collections").and_then(Value::as_array) {
            for c in cols {
                let id = c.get("id").and_then(Value::as_i64);
                let name = c.get("name").and_then(Value::as_str);
                let color = c.get("color").and_then(Value::as_str).unwrap_or("#9b9b9b");
                if let (Some(id), Some(name)) = (id, name) {
                    by_name.insert(name.to_string(), id);
                    order.push((id, name.to_string(), color.to_string()));
                }
            }
        }
        let palette_idx = order.len();
        Collections { by_name, order, palette_idx }
    }

    /// Return the id for `name`, reusing an existing collection by name or
    /// minting a new id in `facet`'s band (one past the band's current max).
    fn ensure(&mut self, facet: Facet, name: &str) -> i64 {
        if let Some(&id) = self.by_name.get(name) {
            return id;
        }
        let id = self.next_in_band(facet);
        let color = PALETTE[self.palette_idx % PALETTE.len()].to_string();
        self.palette_idx += 1;
        self.by_name.insert(name.to_string(), id);
        self.order.push((id, name.to_string(), color));
        id
    }

    fn next_in_band(&self, facet: Facet) -> i64 {
        let (floor, ceiling) = (facet.floor(), facet.ceiling());
        let max_in_band = self
            .order
            .iter()
            .map(|(id, _, _)| *id)
            .filter(|id| *id >= floor && *id < ceiling)
            .max();
        match max_in_band {
            Some(m) => m + 1,
            None => floor,
        }
    }

    fn into_values(self) -> Vec<Value> {
        self.order
            .into_iter()
            .map(|(id, name, color)| json!({ "id": id, "name": name, "color": color }))
            .collect()
    }
}

// --- asset identity --------------------------------------------------------

fn existing_asset_keys(doc: &Value) -> std::collections::HashSet<String> {
    doc.get("assets")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let folder = a.get("folder_path").and_then(Value::as_str)?;
                    let name = a.get("name").and_then(Value::as_str)?;
                    Some(asset_key(folder, name))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn asset_key(folder_path: &str, name: &str) -> String {
    format!("{folder_path}|{name}")
}

/// Parent `res://` dir of a `res://…/file` path.
fn parent_of(res_path: &str) -> String {
    match res_path.rfind('/') {
        Some(i) => res_path[..i].to_string(),
        None => res_path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(pack: &str, cat: &str, fav: bool, tags: &[&str], res: &str, name: &str) -> PlacerAsset {
        PlacerAsset {
            pack: pack.into(),
            category: cat.into(),
            favorite: fav,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            res_path: res.into(),
            name: name.into(),
        }
    }

    #[test]
    fn fresh_library_bands_ids_by_facet() {
        let a = asset(
            "Polygon City",
            "Buildings",
            true,
            &["hero"],
            "res://lib/polygon_city/buildings/Apt.glb",
            "Apt.glb",
        );
        let doc = merge(&empty_library(), &[a]);

        let cols = doc["collections"].as_array().unwrap();
        let id_of = |name: &str| {
            cols.iter()
                .find(|c| c["name"] == name)
                .map(|c| c["id"].as_i64().unwrap())
        };
        assert_eq!(id_of("Polygon City"), Some(1)); // pack band
        assert_eq!(id_of("Buildings"), Some(100)); // category band
        assert_eq!(id_of("Favorites"), Some(1000)); // favorites slot
        assert_eq!(id_of("hero"), Some(1001)); // user-tag band

        let entry = &doc["assets"][0];
        assert_eq!(entry["id"], "res://lib/polygon_city/buildings/Apt.glb");
        assert_eq!(entry["folder_path"], "res://lib/polygon_city/buildings");
        assert_eq!(entry["primary_collection"], 1);
        let tags: Vec<i64> = entry["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_i64().unwrap())
            .collect();
        assert_eq!(tags, vec![1, 100, 1000, 1001]);
    }

    #[test]
    fn merge_is_idempotent_on_reexport() {
        let a = asset("Pack", "Cat", false, &[], "res://b/pack/cat/X.glb", "X.glb");
        let once = merge(&empty_library(), &[a]);
        let a2 = asset("Pack", "Cat", false, &[], "res://b/pack/cat/X.glb", "X.glb");
        let twice = merge(&once, &[a2]);
        assert_eq!(twice["assets"].as_array().unwrap().len(), 1);
        assert_eq!(twice["collections"].as_array().unwrap().len(), 2); // pack + cat, no dupes
    }

    #[test]
    fn reuses_existing_collections_by_name() {
        // Pre-existing library already has a "Buildings" category at a non-default id.
        let existing = json!({
            "version": 3,
            "collections": [{ "id": 150, "name": "Buildings", "color": "#fff" }],
            "assets": [],
            "folders": [],
        });
        let a = asset("NewPack", "Buildings", false, &[], "res://b/newpack/buildings/Y.glb", "Y.glb");
        let doc = merge(&existing, &[a]);
        let cols = doc["collections"].as_array().unwrap();
        let buildings: Vec<_> = cols.iter().filter(|c| c["name"] == "Buildings").collect();
        assert_eq!(buildings.len(), 1);
        assert_eq!(buildings[0]["id"], 150); // reused, not re-minted
        assert_eq!(doc["assets"][0]["tags"][1], 150);
    }

    #[test]
    fn new_pack_appends_after_existing_in_band() {
        let existing = json!({
            "version": 3,
            "collections": [{ "id": 1, "name": "OldPack", "color": "#fff" }],
            "assets": [],
            "folders": [],
        });
        let a = asset("NewPack", "Cat", false, &[], "res://b/newpack/cat/Z.glb", "Z.glb");
        let doc = merge(&existing, &[a]);
        let new_pack = doc["collections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "NewPack")
            .unwrap();
        assert_eq!(new_pack["id"], 2); // 1 was taken
    }

    #[test]
    fn ensure_folder_is_idempotent() {
        let mut doc = empty_library();
        ensure_folder(&mut doc, "res://assets/exported");
        ensure_folder(&mut doc, "res://assets/exported");
        assert_eq!(doc["folders"].as_array().unwrap().len(), 1);
        assert_eq!(doc["folders"][0]["include_subfolders"], true);
    }
}
