//! The app-owned catalog: pure data types, no IO.
//!
//! Pack and Category are modeled as two distinct facets, named explicitly by
//! the native `catalog.json` source. The primary key is the Godot `uid://`,
//! because glTF basenames collide across packs.

use serde::{Deserialize, Serialize};

/// The on-disk catalog. Paths under `fileset` are relative to `library_root`
/// so the library can be relocated without rewriting the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub schema_version: u32,
    pub library_root: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    /// Stable primary key (e.g. `uid://p6p3bf5qgcvb`).
    pub id: String,
    /// Display name, no extension (e.g. `SM_Bld_Apartment_01`).
    pub name: String,
    /// File name with extension (e.g. `SM_Bld_Apartment_01.gltf`).
    pub file_name: String,
    /// Path to the `.gltf`, relative to `library_root`
    /// (e.g. `library/polygon_city/buildings/SM_Bld_Apartment_01.gltf`).
    pub rel_path: String,
    /// Facet 1: which asset pack.
    pub pack: String,
    /// Facet 2: which category.
    pub category: String,
    pub fileset: AssetFileset,
    pub thumb: ThumbMeta,
    pub user: UserMeta,
    #[serde(default)]
    pub animation: AnimationMeta,
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
    pub favorite: bool,
    pub tags: Vec<String>,
}
