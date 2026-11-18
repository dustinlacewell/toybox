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
