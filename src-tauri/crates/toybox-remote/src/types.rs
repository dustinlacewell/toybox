//! The JSON shapes of the server's answers (see docs/server.md).

use serde::{Deserialize, Deserializer, Serialize};
use toybox_core::infra::library::RemoteRole;
use toybox_core::infra::store::ChangesPage;
use toybox_core::ops::transfer::TransferReport;

/// A revision as a number. A server that predates the revision feed sends a string
/// there (its catalog ETag); that reads as 0, which no feed ever has.
fn revision_or_zero<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    Ok(serde_json::Value::deserialize(deserializer)?.as_i64().unwrap_or(0))
}

/// `GET /info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub name: String,
    #[serde(default)]
    pub server_version: String,
    #[serde(default)]
    pub api_version: u32,
    /// The oldest protocol level the server serves (absent from servers before the window).
    #[serde(default)]
    pub min_api_version: Option<u32>,
    /// The oldest Toybox version the server serves.
    #[serde(default)]
    pub min_client_version: Option<String>,
    pub role: RemoteRole,
    /// The library revision (the revision feed is keyed by it).
    #[serde(default, deserialize_with = "revision_or_zero")]
    pub revision: i64,
    /// Identity of the server's library database.
    #[serde(default)]
    pub db_id: String,
    #[serde(default)]
    pub thumb_revision: i64,
}

/// One entry of `GET /tokens`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenView {
    pub id: String,
    pub name: String,
    pub role: RemoteRole,
    pub created_at: u64,
    #[serde(default)]
    pub last_used_at: Option<u64>,
}

/// `POST /tokens`: the new token's record and its secret (shown once).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewToken {
    #[serde(flatten)]
    pub view: TokenView,
    pub token: String,
}

impl std::fmt::Debug for NewToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewToken").field("view", &self.view).field("token", &"<hidden>").finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ThumbEntry {
    pub id: String,
    pub version: String,
}

/// `GET /thumbs`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbList {
    /// The thumbnail revision this list is current to; with `since` only newer pictures are listed.
    #[serde(default)]
    pub thumb_revision: i64,
    #[serde(default)]
    pub thumbs: Vec<ThumbEntry>,
}

/// One file of an upload that is not sent but made on the server (see `Client::link_upload`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UploadLink {
    /// Where it belongs in the upload.
    pub path: String,
    /// `files` (a staged import file) or `package` (a transfer package file).
    pub route: String,
    /// Made from the source the server stores with this content...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// ...or from this other file of the same upload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// `POST /uploads`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadInfo {
    pub id: String,
    #[serde(default)]
    pub max_file_bytes: u64,
    #[serde(default)]
    pub max_upload_bytes: u64,
    #[serde(default)]
    pub max_files: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryError {
    pub index: usize,
    #[serde(default)]
    pub id: Option<String>,
    pub message: String,
}

/// `POST /uploads/{id}/commit`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitReply {
    #[serde(default, deserialize_with = "revision_or_zero")]
    pub revision: i64,
    /// The library revision before the write began.
    #[serde(default)]
    pub base: i64,
    #[serde(default)]
    pub imported: Vec<String>,
    #[serde(default)]
    pub errors: Vec<EntryError>,
    #[serde(default)]
    pub upload_kept: bool,
    /// The rows the write changed, as `/changes` would give them.
    #[serde(default)]
    pub changes: Option<ChangesPage>,
}

/// `POST /uploads/{id}/ingest`: the transfer report plus the revisions and the rows it changed.
#[derive(Debug, Clone, Deserialize)]
pub struct IngestReply {
    #[serde(flatten)]
    pub report: TransferReport,
    #[serde(default, deserialize_with = "revision_or_zero")]
    pub revision: i64,
    #[serde(default)]
    pub base: i64,
    #[serde(default)]
    pub changes: Option<ChangesPage>,
}

/// `POST /assets/metadata`: the new revision, the edited ids, the top-level items
/// they touched and the rows the edit changed.
#[derive(Debug, Clone, Deserialize)]
pub struct MetadataReply {
    #[serde(default, deserialize_with = "revision_or_zero")]
    pub revision: i64,
    #[serde(default)]
    pub base: i64,
    #[serde(default)]
    pub updated: Vec<String>,
    #[serde(default)]
    pub touched: Vec<String>,
    #[serde(default)]
    pub changes: Option<ChangesPage>,
}

/// `POST /assets/delete`.
#[derive(Debug, Clone, Deserialize)]
pub struct DeleteReply {
    #[serde(default, deserialize_with = "revision_or_zero")]
    pub revision: i64,
    #[serde(default)]
    pub base: i64,
    #[serde(default)]
    pub deleted: Vec<String>,
    #[serde(default)]
    pub changes: Option<ChangesPage>,
}
