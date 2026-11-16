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
}

impl Asset {
    /// The model's files, or an error if this asset is not a model.
    pub fn fileset(&self) -> AppResult<&AssetFileset> {
        match &self.content {
            AssetContent::Model { fileset, .. } | AssetContent::Animation { fileset, .. } => Ok(fileset),
            AssetContent::Material { .. } | AssetContent::Audio { .. } => Err(self.not_a_model()),
        }
    }

    /// The model's animation facts, or an error if this asset is not a model.
    /// No Rust command reads animation yet; only tests do.
    #[allow(dead_code)]
    pub fn animation(&self) -> AppResult<&AnimationMeta> {
        match &self.content {
            AssetContent::Model { animation, .. } => Ok(animation),
            AssetContent::Material { .. } | AssetContent::Animation { .. } | AssetContent::Audio { .. } => Err(self.not_a_model()),
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
    pub textures: Vec<String>,
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
