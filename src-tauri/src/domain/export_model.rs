//! Shared export types: requests from the frontend and the report returned.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCopyReq {
    pub asset_ids: Vec<String>,
    pub target_dir: String,
    /// Mirror the pack/category folder layout vs. flatten into the target.
    pub preserve_structure: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportGlbReq {
    pub asset_ids: Vec<String>,
    pub target_dir: String,
    pub preserve_structure: bool,
}

/// Per-asset file format for the asset_placer export.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlacerFormat {
    /// Merged self-contained `.glb` (reuses the GLB bake).
    Glb,
    /// Loose `.gltf` + `.bin` + textures (reuses the copy export).
    Copy,
}

/// One-way publish of selected assets into a Godot project's asset_placer
/// library. Files are written under `<target_dir>/<sub_dir>`; their `res://`
/// ids are formed as `res://<sub_dir>/…`. The addon's `asset_library.json` at
/// `library_json_path` (filesystem path) is created or idempotently merged.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPlacerReq {
    pub asset_ids: Vec<String>,
    /// Filesystem root the files are written under (typically the Godot
    /// project dir).
    pub target_dir: String,
    /// Project-relative subfolder the assets land in, e.g. `assets/exported`.
    /// Also the `res://` prefix. No leading/trailing slashes.
    pub sub_dir: String,
    /// Mirror pack/category folders under `sub_dir` vs. flatten into it.
    pub preserve_structure: bool,
    pub format: PlacerFormat,
    /// Filesystem path of the `asset_library.json` to create or merge.
    pub library_json_path: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    /// Files written, as target-relative paths.
    pub written: Vec<String>,
    /// Assets skipped (e.g. id not found), with a reason.
    pub skipped: Vec<String>,
    /// Non-fatal notes (collision renames, TGA transcodes, etc.).
    pub warnings: Vec<String>,
}

impl ExportReport {
    pub fn write(&mut self, path: impl Into<String>) {
        self.written.push(path.into());
    }
    pub fn skip(&mut self, msg: impl Into<String>) {
        self.skipped.push(msg.into());
    }
    pub fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }
}
