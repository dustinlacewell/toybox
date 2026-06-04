//! Pack identity metadata. A pack is the top-level division of the library
//! (`library/<pack>/...`); its name and color are authored in a `pack.json`
//! inside the pack directory — the source of truth, not derived from the Godot
//! curation file or cached onto assets.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackMeta {
    /// Directory name under `library/` (e.g. `polygon_scifi_space`). Matches an
    /// asset's `pack` field.
    pub slug: String,
    /// Human-facing name (e.g. `Polygon SciFi Space`).
    pub name: String,
    /// Display color as Godot-style `rrggbbaa` hex.
    pub color: String,
}
