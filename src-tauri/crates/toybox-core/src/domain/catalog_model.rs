//! The app-owned catalog: pure data types, no IO.
//!
//! Pack and Category are modeled as two distinct facets, named explicitly by
//! the native `catalog.json` source. The primary key is the Godot `uid://`,
//! because glTF basenames collide across packs.

use serde::{Deserialize, Serialize};

use crate::domain::material::MaterialSet;
use crate::error::{AppError, AppResult};

/// The on-disk catalog. Paths under `fileset` are relative to `library_root`
/// so the library can be relocated without rewriting the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    #[serde(default)]
    pub rigs: Vec<crate::domain::rig::Rig>,
    pub schema_version: u32,
    pub library_root: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rigs: Vec<crate::domain::rig::RigBinding>,
    /// Other file sets of the same thing; the primary is the best available.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<Asset>,
    /// Stable primary key (e.g. `uid://p6p3bf5qgcvb`).
    pub id: String,
    /// Display name, no extension (e.g. `SM_Bld_Apartment_01`).
    pub name: String,
    /// File name with extension (e.g. `SM_Bld_Apartment_01.gltf`) for a model;
    /// the folder name for a material.
    pub file_name: String,
    /// Path relative to `library_root`: the `.gltf` for a model
    /// (e.g. `library/polygon_city/buildings/SM_Bld_Apartment_01.gltf`), the
    /// material's folder for a material.
    pub rel_path: String,
    /// Facet 1: which asset pack.
    pub pack: String,
    /// Facet 2: which category.
    pub category: String,
    #[serde(flatten)]
    pub content: AssetContent,
    pub thumb: ThumbMeta,
    pub user: UserMeta,
    /// Library tags: descriptive keywords that belong to the asset and live in the
    /// library manifest (`tags` of its entry), so everyone who has the library sees
    /// them. Normalised (see `domain::tags`); never part of `user`, which is
    /// personal to one computer. Provenance-derived `source: ...` tags are not here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// The plugin that imported the asset and what it recorded about how.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<AssetOrigin>,
    /// The file format the asset was imported from (`fbx`, `obj`, `wav`, ...):
    /// short, lower-case, no dot. Absent for assets imported before it was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Fingerprint of the asset's content (lower-case hex SHA-256, see
    /// `ops::content_hash`): the same for byte-identical content whatever the id,
    /// name, pack, category or file names. Derived, never part of the manifest:
    /// it is filled from a per-library side cache when a catalog is loaded,
    /// built or served, and absent while that has not been computed yet.
    #[serde(default, rename = "contentHash", skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

/// Normalise a format id: trim, strip leading dots, lower-case; empty is none.
pub fn normalize_format(raw: &str) -> Option<String> {
    let id = raw.trim().trim_start_matches('.').trim().to_lowercase();
    if id.is_empty() { None } else { Some(id) }
}

/// Where an asset came from inside Toybox: the importing plugin's id, that
/// plugin's own record (for example the parameters a generated material was
/// rendered with), and the source files it keeps with the asset (the archive a
/// material was rendered from). The host stores all three without interpreting
/// them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetOrigin {
    pub plugin: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub meta: serde_json::Value,
    /// Source files kept in the asset's own folder. Bare names in the library
    /// manifest; library-relative paths in the catalog, like every other file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

impl AssetOrigin {
    /// Read a manifest entry's `origin`; absent, malformed or without a plugin
    /// id means the entry records none.
    pub fn from_entry(entry: &serde_json::Value) -> Option<AssetOrigin> {
        let origin = entry.get("origin")?;
        let plugin = origin.get("plugin")?.as_str()?.trim();
        if plugin.is_empty() { return None; }
        let files = origin.get("files").and_then(|files| files.as_array()).into_iter().flatten()
            .filter_map(|file| file.as_str()).map(String::from).collect();
        Some(AssetOrigin { plugin: plugin.to_string(), meta: origin.get("meta").cloned().unwrap_or(serde_json::Value::Null), files })
    }
}

/// What an asset is: a glTF model or a set of material maps. Serialized as a
/// `kind` tag next to the shared asset fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AssetContent {
    Audio { audio: crate::domain::audio::AudioMeta },
    Animation {
        fileset: AssetFileset,
        clip: crate::domain::animation::ClipMeta,
    },
    Model {
        fileset: AssetFileset,
        #[serde(default)]
        animation: AnimationMeta,
    },
    Material {
        material: MaterialSet,
    },
    /// An asset of a plugin-defined type: a set of files plus whatever
    /// metadata the type's importer recorded. The host stores, lists and
    /// exports it without interpreting `meta`.
    Plugin {
        /// The declaring plugin's type id (e.g. `skybox`).
        #[serde(rename = "type")]
        type_id: String,
        /// Library-relative paths of the file set.
        files: Vec<String>,
        #[serde(default)]
        meta: serde_json::Value,
    },
}

impl AssetContent {
    /// The library type an asset is listed under: a plugin asset's declared
    /// type id, otherwise the built-in kind.
    pub fn type_id(&self) -> &str {
        match self {
            AssetContent::Model { .. } => "model",
            AssetContent::Material { .. } => "material",
            AssetContent::Animation { .. } => "animation",
            AssetContent::Audio { .. } => "audio",
            AssetContent::Plugin { type_id, .. } => type_id,
        }
    }
}

impl Asset {
    /// The model's files, or an error if this asset is not a model.
    pub fn fileset(&self) -> AppResult<&AssetFileset> {
        match &self.content {
            AssetContent::Model { fileset, .. } | AssetContent::Animation { fileset, .. } => Ok(fileset),
            AssetContent::Material { .. } | AssetContent::Audio { .. } | AssetContent::Plugin { .. } => Err(self.not_a_model()),
        }
    }

    /// The model's animation facts, or an error if this asset is not a model.
    /// No Rust command reads animation yet; only tests do.
    #[allow(dead_code)]
    pub fn animation(&self) -> AppResult<&AnimationMeta> {
        match &self.content {
            AssetContent::Model { animation, .. } => Ok(animation),
            AssetContent::Material { .. } | AssetContent::Animation { .. } | AssetContent::Audio { .. } | AssetContent::Plugin { .. } => Err(self.not_a_model()),
        }
    }

    /// Every library-relative file this item and its variants are made of: a
    /// model's glTF, buffer and images, a material's maps, an audio file, a
    /// plugin item's files, and the source files kept in `origin.files`.
    pub fn collect_files(&self, out: &mut Vec<String>) {
        self.own_files(out);
        for variant in &self.variants {
            variant.collect_files(out);
        }
    }

    /// Like [`Asset::collect_files`] for this item alone, without its variants.
    pub fn own_files(&self, out: &mut Vec<String>) {
        self.content_files(out);
        if let Some(origin) = &self.origin {
            out.extend(origin.files.iter().cloned());
        }
    }

    /// The files the item is shown, played and exported from (no kept source files,
    /// no variants). A remote library downloads exactly these when an item is opened.
    pub fn content_files(&self, out: &mut Vec<String>) {
        match &self.content {
            AssetContent::Audio { audio } => out.push(audio.file.clone()),
            AssetContent::Plugin { files, .. } => out.extend(files.iter().cloned()),
            AssetContent::Model { fileset, .. } | AssetContent::Animation { fileset, .. } => {
                out.push(fileset.gltf.clone());
                out.push(fileset.bin.clone());
                out.extend(fileset.textures.iter().filter(|path| !crate::domain::gltf_uri::is_embedded_path(path)).cloned());
            }
            AssetContent::Material { material } => out.extend(material.maps.iter().map(|map| map.file.clone())),
        }
    }

    /// [`Asset::content_files`] of this item and of its variants.
    pub fn collect_content_files(&self, out: &mut Vec<String>) {
        self.content_files(out);
        for variant in &self.variants {
            variant.collect_content_files(out);
        }
    }

    fn not_a_model(&self) -> AppError {
        AppError::msg(format!("asset {} is not a model", self.id))
    }
}

/// Animation clips an asset's glTF carries (only clips with channels count).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimationMeta {
    pub clip_count: u32,
    pub clip_names: Vec<String>,
}

/// Every file an asset is composed of, each relative to `library_root`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetFileset {
    pub gltf: String,
    pub bin: String,
    /// Unique texture paths referenced by the glTF (shared atlases dedup'd).
    /// Embedded `data:` images are not files and are never listed; caches written
    /// by older versions recorded them as paths, which are dropped when read.
    #[serde(deserialize_with = "files_only")]
    pub textures: Vec<String>,
}

fn files_only<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let mut paths = Vec::<String>::deserialize(deserializer)?;
    paths.retain(|path| !crate::domain::gltf_uri::is_embedded_path(path));
    Ok(paths)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThumbState {
    Missing,
    Queued,
    Rendering,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbMeta {
    pub state: ThumbState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Default for ThumbMeta {
    fn default() -> Self {
        ThumbMeta { state: ThumbState::Missing, error: None }
    }
}

/// App-owned, user-editable metadata. The seeder never writes this back to
/// the source library; re-scans merge-preserve it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<crate::domain::provenance::ImportProvenance>,
    #[serde(default, rename = "previewAssetId", skip_serializing_if = "Option::is_none")]
    pub preview_asset_id: Option<String>,
    pub favorite: bool,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::material::{MapRole, MaterialMap};
    use serde_json::json;

    fn asset(content: AssetContent) -> Asset {
        Asset {
            rigs: Vec::new(),
            variants: Vec::new(),
            id: "uid://a".into(),
            name: "N".into(),
            file_name: "N.gltf".into(),
            rel_path: "library/p/c/N.gltf".into(),
            pack: "p".into(),
            category: "c".into(),
            content,
            thumb: ThumbMeta::default(),
            user: UserMeta::default(),
            tags: Vec::new(),
            origin: None,
            format: None,
            content_hash: None,
        }
    }

    #[test]
    fn model_round_trips_with_kind_tag() {
        let a = asset(AssetContent::Model {
            fileset: AssetFileset { gltf: "g".into(), bin: "b".into(), textures: vec!["t".into()] },
            animation: AnimationMeta { clip_count: 1, clip_names: vec!["Walk".into()] },
        });
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["kind"], "model");
        assert_eq!(v["fileset"]["gltf"], "g");
        assert_eq!(v["animation"]["clipCount"], 1);
        let back: Asset = serde_json::from_value(v).unwrap();
        assert_eq!(back.fileset().unwrap().bin, "b");
        assert_eq!(back.animation().unwrap().clip_names, vec!["Walk"]);
    }

    #[test]
    fn format_round_trips_and_old_manifests_load_without_it() {
        let mut a = asset(AssetContent::Model {
            fileset: AssetFileset { gltf: "g".into(), bin: "b".into(), textures: vec![] },
            animation: AnimationMeta::default(),
        });
        let plain = serde_json::to_value(&a).unwrap();
        assert!(plain.get("format").is_none());
        assert!(serde_json::from_value::<Asset>(plain).unwrap().format.is_none());
        a.format = Some("fbx".into());
        let with = serde_json::to_value(&a).unwrap();
        assert_eq!(with["format"], "fbx");
        assert_eq!(serde_json::from_value::<Asset>(with).unwrap().format.as_deref(), Some("fbx"));
    }

    #[test]
    fn format_ids_are_normalised() {
        assert_eq!(normalize_format(" .FBX "), Some("fbx".into()));
        assert_eq!(normalize_format("..Obj"), Some("obj".into()));
        assert_eq!(normalize_format(" . "), None);
        assert_eq!(normalize_format(""), None);
    }

    #[test]
    fn model_animation_defaults_when_absent() {
        let mut v = serde_json::to_value(asset(AssetContent::Model {
            fileset: AssetFileset { gltf: "g".into(), bin: "b".into(), textures: vec![] },
            animation: AnimationMeta::default(),
        }))
        .unwrap();
        v.as_object_mut().unwrap().remove("animation");
        let back: Asset = serde_json::from_value(v).unwrap();
        assert_eq!(back.animation().unwrap().clip_count, 0);
    }

    #[test]
    fn material_round_trips_and_rejects_model_accessors() {
        let a = asset(AssetContent::Material {
            material: MaterialSet {
                maps: vec![MaterialMap {
                    role: MapRole::Albedo,
                    file: "f.png".into(),
                    channel: None,
                    invert: false,
                }],
                flip_normal_green: true,
            },
        });
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["kind"], "material");
        assert_eq!(v["material"]["flipNormalGreen"], true);
        assert_eq!(v["material"]["maps"][0], json!({"role":"albedo","file":"f.png","invert":false}));
        assert!(v.get("fileset").is_none());
        let back: Asset = serde_json::from_value(v).unwrap();
        assert!(matches!(back.content, AssetContent::Material { .. }));
        let err = back.fileset().unwrap_err().to_string();
        assert_eq!(err, "asset uid://a is not a model");
        assert!(back.animation().is_err());
    }
}
