//! The documents `--json` prints. Field names are camelCase; additions are allowed, renames
//! are not. Each is described in docs/cli.md, "JSON output".

use serde::Serialize;
use serde_json::Value;
use toybox_core::infra::library::LibraryInfo;
use toybox_core::ops::browse::Row;
use toybox_core::ops::transfer::TransferReport;

// ---- overview ------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusDoc {
    pub paths: PathsDoc,
    pub libraries: Vec<LibraryDoc>,
    pub plugins: PluginSummary,
    pub problems: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathsDoc {
    pub app_data: String,
    pub cache: String,
    pub settings: String,
    pub plugins: String,
    /// Whether the app-data folder exists yet.
    pub exists: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDoc {
    pub id: String,
    pub name: String,
    /// `local` or `remote`.
    pub kind: String,
    /// The folder, or the server address.
    pub location: String,
    pub default: bool,
    /// `ok`, `missing`, `read-only`, `offline` or `unreachable`.
    pub state: String,
    pub can_write: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub on: Vec<String>,
    pub off: Vec<String>,
    pub blocked: Vec<String>,
}

#[derive(Serialize)]
pub struct HelpDoc {
    pub commands: Vec<HelpEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelpEntry {
    /// The command as typed after `tbx`, e.g. `library add`.
    pub command: String,
    pub summary: String,
    /// The plugin that adds it; absent for a built-in command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
}

// ---- libraries -----------------------------------------------------------------------

#[derive(Serialize)]
pub struct LibrariesDoc {
    pub libraries: Vec<LibraryDoc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInfoDoc {
    #[serde(flatten)]
    pub library: LibraryDoc,
    pub items: i64,
    pub revision: i64,
    pub types: Vec<TypeCount>,
    /// Files the items are made of; absent for a remote library.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteDoc>,
}

#[derive(Serialize)]
pub struct TypeCount {
    pub id: String,
    pub count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDoc {
    pub url: String,
    pub server_name: String,
    pub role: String,
    pub offline: bool,
    pub cache_bytes: u64,
}

#[derive(Serialize)]
pub struct LibraryChangeDoc {
    pub library: LibraryDoc,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRemovedDoc {
    pub removed: LibraryDoc,
    pub dry_run: bool,
}

#[derive(Serialize)]
pub struct ScanDoc {
    pub library: String,
    pub items: i64,
    pub revision: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckDoc {
    pub library: String,
    pub ok: bool,
    pub items: i64,
    pub files_checked: u64,
    pub problems: Vec<ProblemDoc>,
}

#[derive(Serialize)]
pub struct ProblemDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    pub message: String,
}

#[derive(Serialize)]
pub struct CacheDoc {
    pub library: String,
    pub bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheClearedDoc {
    pub library: String,
    pub freed_bytes: u64,
}

#[derive(Serialize)]
pub struct TokensDoc {
    pub tokens: Vec<toybox_remote::types::TokenView>,
}

// ---- finding and reading -------------------------------------------------------------

#[derive(Serialize)]
pub struct SearchDoc {
    pub total: i64,
    /// Always true: the number is exact.
    pub exact: bool,
    pub offset: i64,
    pub items: Vec<Row>,
}

#[derive(Serialize)]
pub struct CountDoc {
    pub count: i64,
}

#[derive(Serialize)]
pub struct ShowDoc {
    pub items: Vec<ShowItem>,
}

#[derive(Serialize)]
pub struct ShowItem {
    pub library: String,
    /// The item as the app has it.
    pub asset: toybox_core::domain::catalog_model::Asset,
    pub files: Vec<FileDoc>,
}

#[derive(Serialize)]
pub struct FileDoc {
    /// Relative to the library folder.
    pub path: String,
    /// Absent when the file is not on this computer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Serialize)]
pub struct PathsListDoc {
    pub items: Vec<ItemPaths>,
}

#[derive(Serialize)]
pub struct ItemPaths {
    /// The library id (an id found in several libraries is listed for each).
    pub library: String,
    pub id: String,
    /// The main file.
    pub path: String,
    /// Every file (with `--all`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct TypesDoc {
    pub types: Vec<TypeDoc>,
}

#[derive(Serialize)]
pub struct TypeDoc {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    pub native: bool,
    pub count: i64,
    /// Its plugin is off: the app hides the type.
    pub hidden: bool,
}

#[derive(Serialize)]
pub struct FacetsDoc {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub facets: toybox_core::infra::store::browse::FacetCounts,
}

// ---- tags, details, favourites, collections ------------------------------------------

#[derive(Serialize)]
pub struct TagsDoc {
    pub tags: Vec<toybox_core::infra::store::browse::Count>,
}

/// The result of a change to items (`tag`, `edit`, `favorite`, `collection`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeDoc {
    pub library: String,
    pub dry_run: bool,
    /// Ids that changed (or would).
    pub changed: Vec<String>,
    /// Ids that already were as asked.
    pub unchanged: Vec<String>,
}

#[derive(Serialize)]
pub struct CollectionsDoc {
    pub collections: Vec<toybox_core::infra::store::browse::CollectionCount>,
}

// ---- import, export, move, delete ----------------------------------------------------

#[derive(Serialize)]
pub struct ImporterListDoc {
    pub importers: Vec<ImporterDoc>,
}

#[derive(Serialize)]
pub struct ImporterDoc {
    pub name: String,
    pub plugin: String,
    pub summary: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDoc {
    pub library: String,
    pub dry_run: bool,
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct ExporterListDoc {
    pub exporters: Vec<ExporterDoc>,
}

#[derive(Serialize)]
pub struct ExporterDoc {
    pub name: String,
    /// The plugin that provides it; absent for the built-in `copy` and `glb`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    pub summary: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDoc {
    pub library: String,
    pub target: String,
    /// Only planned: nothing was written.
    pub preview: bool,
    pub written: Vec<String>,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferDoc {
    pub from: String,
    pub to: String,
    /// `copy` or `move`.
    pub mode: String,
    pub dry_run: bool,
    pub copied: usize,
    pub replaced: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Items that a move removed from the source.
    pub removed: usize,
    #[serde(flatten)]
    pub report: TransferReport,
    /// For a dry run: the ids that would be transferred.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub would_transfer: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteDoc {
    pub library: String,
    pub dry_run: bool,
    /// Ids that are gone (or would be).
    pub deleted: Vec<String>,
    /// Items left in the library.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<i64>,
}

#[derive(Serialize)]
pub struct ThumbStatusDoc {
    pub library: String,
    pub pending: usize,
    pub items: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbsClearDoc {
    pub library: String,
    pub dry_run: bool,
    /// Items whose thumbnails were discarded (`null` when all of them).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleared: Option<usize>,
    pub all: bool,
}

// ---- plugins and settings ------------------------------------------------------------

#[derive(Serialize)]
pub struct PluginsDoc {
    pub plugins: Vec<PluginDoc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDoc {
    pub id: String,
    pub name: String,
    pub version: String,
    /// `on`, `off` or `blocked`.
    pub state: String,
    /// Why it is blocked.
    pub blocked_by: Vec<String>,
    /// What it provides: `types`, `importers`, `exporter`, `commands`, `panel`, `rigs`, ...
    pub provides: Vec<String>,
    /// Command-line declarations that are not mounted, and why.
    pub problems: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfoDoc {
    #[serde(flatten)]
    pub plugin: PluginDoc,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub path: String,
    pub dependencies: Value,
    pub required_by: Vec<String>,
    pub types: Vec<String>,
    pub importers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exporter: Option<String>,
    pub commands: Vec<String>,
    pub permissions: Value,
    pub settings: Vec<PluginSettingDoc>,
}

#[derive(Serialize)]
pub struct PluginSettingDoc {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    pub value: Value,
    pub default: Value,
}

#[derive(Serialize)]
pub struct PluginSwitchDoc {
    pub id: String,
    pub enabled: bool,
    /// Plugins whose switch changed (including this one).
    pub changed: Vec<String>,
}

#[derive(Serialize)]
pub struct PluginInstalledDoc {
    pub id: String,
    pub name: String,
    pub version: String,
    pub path: String,
    pub linked: bool,
}

#[derive(Serialize)]
pub struct PluginRemovedDoc {
    pub id: String,
}

#[derive(Serialize)]
pub struct PluginSettingValueDoc {
    pub plugin: String,
    pub key: String,
    pub value: Value,
}

#[derive(Serialize)]
pub struct ConfigDoc {
    pub settings: serde_json::Map<String, Value>,
}

#[derive(Serialize)]
pub struct ConfigValueDoc {
    pub key: String,
    pub value: Value,
}

// ---- looking inside ------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugPathsDoc {
    pub app_data: String,
    pub cache: String,
    pub settings: String,
    pub plugins: String,
    pub remote_files: String,
    pub plugin_scratch: String,
    pub browse_index: String,
    /// Where access tokens are kept.
    pub secrets: String,
    pub libraries: Vec<DebugLibraryPaths>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugLibraryPaths {
    pub id: String,
    pub name: String,
    pub root: String,
    pub database: String,
    pub user_database: String,
    pub app_data: String,
    pub thumbnails: String,
}

#[derive(Serialize)]
pub struct SqlDoc {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub truncated: bool,
}

/// Convert a library for the documents above.
pub fn library_doc(info: &LibraryInfo, items: Option<i64>) -> LibraryDoc {
    let state = match (&info.remote, info.available) {
        (Some(_), false) => "unreachable",
        (Some(remote), true) if remote.offline => "offline",
        (None, false) => "missing",
        (_, true) if !info.can_write => "read-only",
        _ => "ok",
    };
    LibraryDoc {
        id: info.id.clone(),
        name: info.name.clone(),
        kind: match info.kind {
            toybox_core::infra::registry::LibraryKind::Local => "local".into(),
            toybox_core::infra::registry::LibraryKind::Remote => "remote".into(),
        },
        location: info.remote.as_ref().map(|remote| remote.url.clone()).unwrap_or_else(|| info.root.clone()),
        default: info.is_default,
        state: state.to_string(),
        can_write: info.can_write,
        items,
    }
}
