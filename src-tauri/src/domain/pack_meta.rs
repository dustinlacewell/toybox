//! Legacy source metadata for plugin compatibility. Managed library grouping
//! uses provenance and tags; these identifiers never locate managed media.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackMeta {
    /// Legacy source identifier (e.g. `polygon_scifi_space`). Matches an
    /// asset's `pack` field.
    pub slug: String,
    /// Human-facing name (e.g. `Polygon SciFi Space`).
    pub name: String,
    /// Display color as Godot-style `rrggbbaa` hex.
    pub color: String,
}
