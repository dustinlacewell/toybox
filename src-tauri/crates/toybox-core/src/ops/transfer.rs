//! Transferring assets between libraries.
//!
//! A transfer is two halves joined by a neutral **package**, so either side can
//! later be a remote library:
//!
//! * [`export_package`] reads items out of a source library and describes them:
//!   each item's manifest entry exactly as the library records it (so `format`,
//!   `provenance`, `origin`, variants and everything else survive), the files it
//!   needs with their library-relative paths, sizes and SHA-256 digests, the
//!   personal metadata the source's cache holds for it, and its thumbnails.
//!   The package holds no bytes; they are fetched by path through a reader.
//! * [`ingest_package`] writes a package into a target library. The reader it is
//!   given supplies the bytes (`local_reader` for a folder on this machine, an
//!   HTTP client for a remote library). Files are staged exactly as an importer
//!   stages them and committed through [`super::import::merge_entries`], so
//!   storage keys, shared `_resources` images, validation and the cache update
//!   are those of a normal import.
//!
//! [`transfer_between`] joins the two for two local libraries and adds the move
//! step (verify the target has the item, then delete it from the source through
//! [`super::delete`]).
//!
//! What an item brings: see `docs/libraries.md` ("Transferring").

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::domain::catalog_model::{Asset, UserMeta};
use crate::domain::seed::{self, SeedContent, SeedEntry};
use crate::domain::{gltf_parse, gltf_uri, paths};
use crate::error::{AppError, AppResult};
use crate::infra::registry::LibraryHandle;
use crate::infra::store::{Delta, ItemPut, Store};
use crate::infra::{fsio, library as library_rules};

use super::import::merge_entries;
#[cfg(test)]
use super::catalog::{load_catalog_inner, save_catalog_inner};

/// Version of the package shape. A package of another version is refused.
pub const PACKAGE_VERSION: u32 = 1;
/// Items are committed in groups of this many (one manifest publish per group).
const CHUNK: usize = 20;
/// Staging packs are named `_import_t_<hash of the item id>`: `_import_` makes
/// the storage layer treat them as private staging, and the item id keeps the
/// storage key of an item stable between transfers.
const STAGE_PREFIX: &str = "_import_t_";
/// Images shared by the items of one group, staged once under their digest.
const STAGE_RESOURCES: &str = "_import_t_res";
/// Package paths below this prefix are thumbnails, not library files.
const THUMB_PREFIX: &str = "thumbs/";

static INGEST: Mutex<()> = Mutex::new(());

// --- Package ------------------------------------------------------------------

/// A file an item needs. `path` is relative to the source library root (always
/// `library/...`), or `thumbs/<name>.png` for a thumbnail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageThumb {
    /// The asset or variant the picture belongs to.
    pub id: String,
    pub file: PackageFile,
}

/// One top-level item with its variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageAsset {
    pub id: String,
    pub name: String,
    /// The library type: `model`, `material`, `animation`, `audio` or a plugin type id.
    pub kind: String,
    /// The manifest entry as the source library records it, variants included.
    pub entry: Value,
    /// Every file of the item and of its variants (each path once).
    pub files: Vec<PackageFile>,
    /// The personal metadata (`user`) the source cache holds, by asset or variant id.
    /// It is not part of the library; the target's cache receives it.
    #[serde(default)]
    pub user: BTreeMap<String, Value>,
    #[serde(default)]
    pub thumbs: Vec<PackageThumb>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemFailure {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferPackage {
    pub version: u32,
    pub source_library: String,
    pub assets: Vec<PackageAsset>,
    /// Requested items that could not be described (unknown id, missing file...).
    #[serde(default)]
    pub failed: Vec<ItemFailure>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ExportOptions {
    /// Also take the animation clips extracted from the selected models. Off by
    /// default: a clip is an item of its own.
    pub include_clips: bool,
}

/// Supplies the bytes of a package path (a library file or a thumbnail).
pub type ReadFile<'a> = &'a dyn Fn(&str) -> AppResult<Box<dyn Read>>;

// --- Options and report ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnConflict {
    /// An item whose id already exists in the target is left alone.
    #[default]
    Skip,
    /// It is replaced by the incoming one (the target keeps its own personal metadata).
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferMode {
    Copy,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Copied,
    Replaced,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub outcome: Outcome,
    /// Why it was skipped or failed, or a note on one that was copied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// A move deleted the item from the source.
    #[serde(default)]
    pub source_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferReport {
    pub items: Vec<TransferItem>,
}

impl TransferReport {
    pub fn count(&self, outcome: Outcome) -> usize {
        self.items.iter().filter(|item| item.outcome == outcome).count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct IngestOptions {
    pub on_conflict: OnConflict,
}

#[derive(Debug, Clone, Copy)]
pub struct TransferOptions {
    pub mode: TransferMode,
    pub on_conflict: OnConflict,
    pub include_clips: bool,
}

// --- Export ---------------------------------------------------------------------

/// Describe `ids` of `source` as a package. An item that cannot be described
/// goes to `failed`; the others are unaffected.
pub fn export_package(source: &LibraryHandle, ids: &[String], options: ExportOptions) -> AppResult<TransferPackage> {
    let store = Store::for_handle(source);
    if !store.manifest_present() && !source.is_remote() {
        return Err(AppError::msg(format!("not a Toybox library: no manifest in {}", source.root.display())));
    }

    let mut wanted: Vec<String> = Vec::new();
    for id in ids {
        if !wanted.contains(id) {
            wanted.push(id.clone());
        }
    }
    let mut entries: HashMap<String, Value> = store.load_entries(&wanted)?.into_iter().filter_map(|entry| Some((entry["id"].as_str()?.to_string(), entry))).collect();
    if options.include_clips {
        for id in wanted.clone() {
            let Some(entry) = entries.get(&id) else { continue };
            let mut owners: Vec<String> = vec![id.clone()];
            owners.extend(entry["variants"].as_array().into_iter().flatten().filter_map(|variant| variant["id"].as_str().map(String::from)));
            // The clips cut from this item, when each is an item of the library of its own.
            let clips = store.clip_ids_cut_from(&owners)?;
            let present = store.present_items(&clips)?;
            for clip_id in clips.into_iter().filter(|clip_id| present.contains(clip_id)) {
                if !wanted.iter().any(|w| *w == clip_id) {
                    wanted.push(clip_id);
                }
            }
        }
        let missing: Vec<String> = wanted.iter().filter(|id| !entries.contains_key(*id)).cloned().collect();
        for entry in store.load_entries(&missing)? {
            if let Some(id) = entry["id"].as_str().map(String::from) {
                entries.insert(id, entry);
            }
        }
    }
    // The personal data this computer holds for the items: their rows only.
    let cache: HashMap<String, Asset> = store.load_top_assets(&wanted)?.into_iter().map(|asset| (asset.id.clone(), asset)).collect();

    let mut digests: HashMap<String, (u64, String)> = HashMap::new();
    let mut package = TransferPackage { version: PACKAGE_VERSION, source_library: source.id.clone(), assets: Vec::new(), failed: Vec::new() };
    for id in wanted {
        let Some(entry) = entries.get(&id) else {
            package.failed.push(ItemFailure { id: id.clone(), name: id, kind: "model".into(), reason: "No longer in the source library".into() });
            continue;
        };
        match package_asset(source, entry, cache.get(&id), &mut digests) {
            Ok(asset) => package.assets.push(asset),
            Err(error) => {
                let (name, kind) = describe(entry);
                package.failed.push(ItemFailure { id, name, kind, reason: error.to_string() });
            }
        }
    }
    Ok(package)
}

/// The personal data (`user`) a cached asset and its variants hold, by id.
fn user_of(asset: &Asset) -> BTreeMap<String, Value> {
    let mut user = BTreeMap::new();
    if let Ok(value) = serde_json::to_value(&asset.user) {
        user.insert(asset.id.clone(), value);
    }
    for variant in &asset.variants {
        if let Ok(value) = serde_json::to_value(&variant.user) {
            user.insert(variant.id.clone(), value);
        }
    }
    user
}

fn package_asset(source: &LibraryHandle, entry: &Value, cached: Option<&Asset>, digests: &mut HashMap<String, (u64, String)>) -> AppResult<PackageAsset> {
    let parsed = seed::parse_entry(entry)?;
    let mut paths: Vec<String> = Vec::new();
    for item in std::iter::once(&parsed).chain(&parsed.variants) {
        for path in item_paths(&source.root, item)? {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        files.push(describe_file(&source.root, &path, digests)?);
    }
    let ids: Vec<&str> = std::iter::once(parsed.id.as_str()).chain(parsed.variants.iter().map(|v| v.id.as_str())).collect();

    let user = cached.map(user_of).unwrap_or_default();
    let mut thumbs = Vec::new();
    for id in ids {
        let path = source.thumb_path(id);
        if let Ok(meta) = std::fs::metadata(&path) {
            if let (true, Some(name), Ok(sha256)) = (meta.is_file(), path.file_name().and_then(|n| n.to_str()), hash_path(&path)) {
                thumbs.push(PackageThumb { id: id.to_string(), file: PackageFile { path: format!("{THUMB_PREFIX}{name}"), size: meta.len(), sha256 } });
            }
        }
    }
    let (name, kind) = describe(entry);
    Ok(PackageAsset { id: parsed.id, name, kind, entry: entry.clone(), files, user, thumbs })
}

/// Every library file one item (not its variants) needs, source-relative.
fn item_paths(root: &Path, item: &SeedEntry) -> AppResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    match &item.content {
        SeedContent::Model { gltf_rel, .. } | SeedContent::AnimationSource { gltf_rel, .. } => {
            let bytes = fsio::read_bytes(&paths::abs_under_root(&root.to_string_lossy(), gltf_rel))?;
            let doc = gltf_parse::parse(&String::from_utf8_lossy(&bytes))?;
            let deps = scene_dependencies(&doc, gltf_rel)?;
            out.push(gltf_rel.clone());
            out.extend(deps.buffer.map(|(_, path)| path));
            out.extend(deps.images.into_iter().map(|(_, path)| path));
        }
        SeedContent::Material { material } => out.extend(material.textures()),
        SeedContent::Audio { file } => out.push(file.clone()),
        SeedContent::Plugin { files, .. } => out.extend(files.iter().cloned()),
    }
    if let Some(origin) = &item.origin {
        out.extend(origin.files.iter().cloned());
    }
    let mut unique: Vec<String> = Vec::new();
    for path in out {
        safe_rel(&path)?;
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    Ok(unique)
}

/// The external files a glTF names, resolved against the glTF's own folder.
struct SceneDeps {
    buffer: Option<(usize, String)>,
    images: Vec<(usize, String)>,
}

fn scene_dependencies(doc: &Value, gltf_rel: &str) -> AppResult<SceneDeps> {
    if doc["buffers"].as_array().is_some_and(|buffers| buffers.len() > 1) {
        return Err(AppError::msg(format!("Cannot transfer a scene with several buffers: {gltf_rel}")));
    }
    let parent = gltf_rel.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let resolve = |uri: &str| gltf_uri::resolve_library_file(parent, uri);
    let mut deps = SceneDeps { buffer: None, images: Vec::new() };
    for reference in gltf_parse::buffer_uris(doc) {
        if let Some(path) = resolve(&reference.uri)? {
            deps.buffer = Some((reference.index, path));
        }
    }
    for reference in gltf_parse::image_uris(doc) {
        if let Some(path) = resolve(&reference.uri)? {
            deps.images.push((reference.index, path));
        }
    }
    Ok(deps)
}

fn describe_file(root: &Path, rel: &str, digests: &mut HashMap<String, (u64, String)>) -> AppResult<PackageFile> {
    safe_rel(rel)?;
    if let Some((size, sha256)) = digests.get(rel) {
        return Ok(PackageFile { path: rel.to_string(), size: *size, sha256: sha256.clone() });
    }
    let path = paths::abs_under_root(&root.to_string_lossy(), rel);
    let size = std::fs::metadata(&path).map_err(|error| AppError::file("Read", &path, error))?.len();
    let sha256 = hash_path(&path)?;
    digests.insert(rel.to_string(), (size, sha256.clone()));
    Ok(PackageFile { path: rel.to_string(), size, sha256 })
}

fn hash_path(path: &Path) -> AppResult<String> {
    let mut file = std::fs::File::open(path).map_err(|error| AppError::file("Read", path, error))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// A library-relative path a package may name: `library/...` made of plain segments.
fn safe_rel(rel: &str) -> AppResult<()> {
    let plain = rel.starts_with("library/")
        && !rel.contains(['\\', ':'])
        && rel.split('/').all(|part| !part.is_empty() && part != "." && part != "..");
    if plain { Ok(()) } else { Err(AppError::msg(format!("Not a library path: {rel}"))) }
}

/// Display name and library type of a manifest entry, without parsing it.
fn describe(entry: &Value) -> (String, String) {
    let kind = match entry["kind"].as_str().unwrap_or("model") {
        "plugin" => entry["type"].as_str().unwrap_or("plugin").to_string(),
        other => other.to_string(),
    };
    let name = entry["name"].as_str().map(String::from)
        .or_else(|| entry["file"].as_str().map(|file| file.rsplit(['/', '\\']).next().unwrap_or(file).trim_end_matches(".gltf").to_string()))
        .unwrap_or_else(|| entry["id"].as_str().unwrap_or("?").to_string());
    (name, kind)
}

/// A reader for a library on this machine. Paths are confined to its root; a
/// `thumbs/` path reads from its thumbnail folder.
pub fn local_reader(library: &LibraryHandle) -> impl Fn(&str) -> AppResult<Box<dyn Read>> + '_ {
    move |rel: &str| {
        let path = if let Some(name) = rel.strip_prefix(THUMB_PREFIX) {
            if name.is_empty() || name.contains(['/', '\\', ':']) || name.starts_with('.') {
                return Err(AppError::msg(format!("Not a thumbnail path: {rel}")));
            }
            library.thumbs_dir.join(name)
        } else {
            safe_rel(rel)?;
            let path = paths::abs_under_root(&library.root_str(), rel);
            let inside = path.canonicalize().ok().zip(library.root.canonicalize().ok()).is_some_and(|(file, root)| file.starts_with(root));
            if !inside {
                return Err(AppError::msg(format!("Missing or outside the library: {rel}")));
            }
            path
        };
        let file = std::fs::File::open(&path).map_err(|error| AppError::file("Read", &path, error))?;
        Ok(Box::new(file) as Box<dyn Read>)
    }
}

// --- Ingest ---------------------------------------------------------------------

/// Write `package` into `target`. Every item is handled on its own: one that
/// cannot be copied is reported as failed and the others still land.
pub fn ingest_package(
    target: &LibraryHandle,
    package: &TransferPackage,
    options: IngestOptions,
    read: ReadFile,
    progress: &dyn Fn(TransferProgress),
) -> AppResult<TransferReport> {
    if package.version != PACKAGE_VERSION {
        return Err(AppError::msg(format!("Unsupported transfer package version {}", package.version)));
    }
    if !library_rules::is_writable(&target.root) {
        return Err(AppError::msg("The target library is read-only"));
    }
    let _guard = INGEST.lock().map_err(|_| AppError::msg("Transfer lock was poisoned"))?;

    // Which of the package's ids the target already uses, and by which top-level item
    // (asked of the database for these ids only).
    let store = Store::for_handle(target);
    super::catalog::ready_for_write(target)?;
    let mut package_ids: Vec<String> = Vec::new();
    for asset in &package.assets {
        package_ids.push(asset.id.clone());
        package_ids.extend(asset.entry["variants"].as_array().into_iter().flatten().filter_map(|variant| variant["id"].as_str().map(String::from)));
    }
    let owners: HashMap<String, String> = store.id_owners(&package_ids)?;

    let total = package.assets.len() + package.failed.len();
    let mut items: Vec<TransferItem> = package.assets.iter().map(|asset| TransferItem {
        id: asset.id.clone(), name: asset.name.clone(), kind: asset.kind.clone(), outcome: Outcome::Failed, reason: None, source_removed: false,
    }).collect();
    let mut replacing: HashSet<usize> = HashSet::new();
    let mut todo: Vec<usize> = Vec::new();
    for (index, asset) in package.assets.iter().enumerate() {
        match conflict(asset, &owners) {
            Conflict::None => todo.push(index),
            Conflict::Exists if options.on_conflict == OnConflict::Replace => {
                replacing.insert(index);
                todo.push(index);
            }
            Conflict::Exists => {
                items[index].outcome = Outcome::Skipped;
                items[index].reason = Some("Already in the target library".into());
            }
            Conflict::Foreign(reason) => items[index].reason = Some(reason),
        }
    }
    let mut done = package.failed.len() + items.iter().filter(|item| item.outcome == Outcome::Skipped || item.reason.is_some()).count();
    progress(TransferProgress { done, total });

    let mut committed: Vec<usize> = Vec::new();
    for chunk in todo.chunks(CHUNK) {
        clear_staging(&target.root);
        let mut written = HashMap::new();
        let mut staged: Vec<(usize, Value)> = Vec::new();
        for &index in chunk {
            match stage_asset(&target.root, &package.assets[index], read, &mut written) {
                Ok(entry) => staged.push((index, entry)),
                Err(error) => items[index].reason = Some(error.to_string()),
            }
        }
        let mut landed: Vec<usize> = Vec::new();
        if !staged.is_empty() {
            match merge_entries(target, staged.iter().map(|(_, entry)| entry.clone()).collect()) {
                Ok(merged) => {
                    for (index, _) in &staged {
                        if merged.assets.iter().any(|asset| asset.id == package.assets[*index].id) {
                            landed.push(*index);
                        } else {
                            items[*index].reason = Some("Not found in the target after it was committed".into());
                        }
                    }
                }
                Err(batch) if staged.len() == 1 => items[staged[0].0].reason = Some(batch.to_string()),
                Err(_) => {
                    // One bad item must not sink its neighbours: commit them one by one.
                    for (index, _) in &staged {
                        clear_staging(&target.root);
                        let result = stage_asset(&target.root, &package.assets[*index], read, &mut HashMap::new())
                            .and_then(|entry| merge_entries(target, vec![entry]));
                        match result {
                            Ok(_) => landed.push(*index),
                            Err(error) => items[*index].reason = Some(error.to_string()),
                        }
                    }
                }
            }
        }
        clear_staging(&target.root);
        for index in landed {
            items[index].outcome = if replacing.contains(&index) { Outcome::Replaced } else { Outcome::Copied };
            committed.push(index);
        }
        done += chunk.len();
        progress(TransferProgress { done: done.min(total), total });
    }

    carry_thumbnails(target, package, &committed, read);
    if let Err(error) = carry_personal_metadata(target, package, &committed, &items) {
        for &index in &committed {
            if items[index].outcome == Outcome::Copied {
                items[index].reason = Some(format!("Copied, but its favourites, tags and collections could not be carried: {error}"));
            }
        }
    }

    let mut report = TransferReport { items };
    for failure in &package.failed {
        report.items.push(TransferItem {
            id: failure.id.clone(), name: failure.name.clone(), kind: failure.kind.clone(),
            outcome: Outcome::Failed, reason: Some(failure.reason.clone()), source_removed: false,
        });
    }
    Ok(report)
}

enum Conflict {
    None,
    /// The item's own id is already a top-level item of the target.
    Exists,
    /// The id of the item or of a variant belongs to a different item there.
    Foreign(String),
}

fn conflict(asset: &PackageAsset, owners: &HashMap<String, String>) -> Conflict {
    let mut ids = vec![asset.id.as_str()];
    ids.extend(asset.entry["variants"].as_array().into_iter().flatten().filter_map(|variant| variant["id"].as_str()));
    for id in ids {
        if let Some(owner) = owners.get(id).filter(|owner| **owner != asset.id) {
            return Conflict::Foreign(format!("The id {id} is already used by another item ({owner}) in the target library"));
        }
    }
    if owners.contains_key(&asset.id) { Conflict::Exists } else { Conflict::None }
}

/// `library/_import_t_*` folders are this module's staging; nothing else uses them.
fn clear_staging(root: &Path) {
    let Ok(folders) = std::fs::read_dir(root.join("library")) else { return };
    for folder in folders.flatten() {
        if folder.file_name().to_string_lossy().starts_with(STAGE_PREFIX) {
            let _ = std::fs::remove_dir_all(folder.path());
        }
    }
}

fn sha_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Stage one item as an importer would: its entry without a `storagePath`, under
/// a private pack, with its files in the layout the storage layer expects. The
/// item's own pack and category ride along in `transferOf`.
fn stage_asset(root: &Path, asset: &PackageAsset, read: ReadFile, written: &mut HashMap<String, String>) -> AppResult<Value> {
    let files: HashMap<&str, &PackageFile> = asset.files.iter().map(|file| (file.path.as_str(), file)).collect();
    let parsed = seed::parse_entry(&asset.entry)?;
    let pack = format!("{STAGE_PREFIX}{}", &sha_hex(asset.id.as_bytes())[..12]);
    let mut staged = asset.entry.clone();
    let mut stager = Stager { root, files: &files, read, written, pack: &pack };
    stager.item(&mut staged, &parsed)?;
    if let Some(variants) = staged.get_mut("variants").and_then(Value::as_array_mut) {
        for (variant, parsed) in variants.iter_mut().zip(&parsed.variants) {
            stager.item(variant, parsed)?;
        }
    }
    Ok(staged)
}

struct Stager<'a> {
    root: &'a Path,
    files: &'a HashMap<&'a str, &'a PackageFile>,
    read: ReadFile<'a>,
    written: &'a mut HashMap<String, String>,
    pack: &'a str,
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl<'a> Stager<'a> {
    fn item(&mut self, item: &mut Value, parsed: &SeedEntry) -> AppResult<()> {
        let own = json!({
            "pack": item["pack"].as_str().unwrap_or("imported"),
            "category": item["category"].as_str().unwrap_or("uncategorized"),
        });
        let object = item.as_object_mut().ok_or_else(|| AppError::msg("Manifest entry is not an object"))?;
        object.remove("storagePath");
        object.insert("transferOf".into(), own);
        item["pack"] = json!(self.pack);
        item["category"] = json!("t");
        let pack = self.pack.to_string();
        let mut kept_in = String::new();
        match &parsed.content {
            SeedContent::Model { gltf_rel, .. } | SeedContent::AnimationSource { gltf_rel, .. } => {
                let file = basename(gltf_rel).to_string();
                item["file"] = json!(file);
                let dir = format!("library/{pack}/t");
                let stem = file.strip_suffix(".gltf").unwrap_or(&file).to_string();
                let bytes = self.fetch(gltf_rel)?;
                let mut doc = gltf_parse::parse(&String::from_utf8_lossy(&bytes))?;
                let deps = scene_dependencies(&doc, gltf_rel)?;
                if let Some((index, path)) = deps.buffer {
                    let bin = format!("{stem}.bin");
                    self.stage_from(&format!("{dir}/{bin}"), &path)?;
                    gltf_parse::set_uri(&mut doc, "buffers", index, &bin)?;
                }
                for (index, path) in deps.images {
                    // Images are stored by content, so one staged copy serves every item.
                    let digest = self.file(&path)?.sha256.clone();
                    let extension = Path::new(&path).extension().and_then(|e| e.to_str()).unwrap_or("bin").to_lowercase();
                    let name = format!("{digest}.{extension}");
                    self.stage_from(&format!("library/{STAGE_RESOURCES}/{name}"), &path)?;
                    gltf_parse::set_uri(&mut doc, "images", index, &format!("../../{STAGE_RESOURCES}/{name}"))?;
                }
                self.stage_bytes(&format!("{dir}/{file}"), serde_json::to_vec_pretty(&doc)?)?;
                kept_in = dir;
            }
            SeedContent::Material { material } => {
                let name = item["name"].as_str().ok_or_else(|| AppError::msg("Material entry has no name"))?.to_string();
                let dir = format!("library/{pack}/materials/{name}");
                let declared: Vec<String> = item["maps"].as_array().into_iter().flatten().filter_map(|map| map["file"].as_str().map(String::from)).collect();
                for (declared, resolved) in declared.iter().zip(&material.maps) {
                    self.stage_from(&format!("{dir}/{declared}"), &resolved.file)?;
                }
                kept_in = dir;
            }
            SeedContent::Audio { file } => {
                let name = basename(file).to_string();
                item["file"] = json!(name);
                self.stage_from(&format!("library/{pack}/t/{name}"), file)?;
            }
            SeedContent::Plugin { type_id, files, .. } => {
                let name = item["name"].as_str().ok_or_else(|| AppError::msg("Plugin entry has no name"))?.to_string();
                let dir = format!("library/{pack}/{type_id}/{name}");
                for source in files {
                    self.stage_from(&format!("{dir}/{}", basename(source)), source)?;
                }
            }
        }
        // Source files the importer kept sit in the item's own folder, by bare name.
        if let Some(origin) = &parsed.origin {
            for source in &origin.files {
                self.stage_from(&format!("{kept_in}/{}", basename(source)), source)?;
            }
        }
        Ok(())
    }

    fn file(&self, path: &str) -> AppResult<&'a PackageFile> {
        self.files.get(path).copied().ok_or_else(|| AppError::msg(format!("The package does not list {path}")))
    }

    fn fetch(&mut self, path: &str) -> AppResult<Vec<u8>> {
        let file = self.file(path)?;
        let mut bytes = Vec::with_capacity(file.size as usize);
        (self.read)(path)?.read_to_end(&mut bytes)?;
        if bytes.len() as u64 != file.size || sha_hex(&bytes) != file.sha256 {
            return Err(AppError::msg(format!("{path} changed while it was being copied")));
        }
        Ok(bytes)
    }

    /// Stage the package file `path` at `dest`, checking size and digest as it is written.
    fn stage_from(&mut self, dest: &str, path: &str) -> AppResult<()> {
        let file = self.file(path)?;
        if self.already(dest, &file.sha256)? {
            return Ok(());
        }
        let target = paths::abs_under_root(&self.root.to_string_lossy(), dest);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| AppError::file("Create directory", parent, error))?;
        }
        let mut source = (self.read)(path)?;
        let mut out = std::fs::File::create(&target).map_err(|error| AppError::file("Write", &target, error))?;
        let mut hash = Sha256::new();
        let mut size = 0u64;
        let mut buffer = vec![0u8; 64 * 1024];
        let copied = (|| -> AppResult<()> {
            loop {
                let count = source.read(&mut buffer)?;
                if count == 0 {
                    return Ok(out.flush()?);
                }
                hash.update(&buffer[..count]);
                size += count as u64;
                out.write_all(&buffer[..count])?;
            }
        })();
        drop(out);
        let digest = format!("{:x}", hash.finalize());
        if let Err(error) = copied {
            let _ = std::fs::remove_file(&target);
            return Err(error);
        }
        if size != file.size || digest != file.sha256 {
            let _ = std::fs::remove_file(&target);
            return Err(AppError::msg(format!("{path} changed while it was being copied")));
        }
        self.written.insert(dest.to_string(), digest);
        Ok(())
    }

    fn stage_bytes(&mut self, dest: &str, bytes: Vec<u8>) -> AppResult<()> {
        let digest = sha_hex(&bytes);
        if self.already(dest, &digest)? {
            return Ok(());
        }
        fsio::write_bytes(&paths::abs_under_root(&self.root.to_string_lossy(), dest), &bytes)?;
        self.written.insert(dest.to_string(), digest);
        Ok(())
    }

    /// Whether `dest` is already staged with this content. The same name with
    /// other content is a collision of two files of one item set (for example
    /// two variants keeping different files of the same name).
    fn already(&self, dest: &str, digest: &str) -> AppResult<bool> {
        match self.written.get(dest) {
            Some(prior) if prior == digest => Ok(true),
            Some(_) => Err(AppError::msg(format!("Two files of this item would both be stored as {}", basename(dest)))),
            None => Ok(false),
        }
    }
}

/// Copy the pictures the source already rendered. They are only a cache, so any
/// problem is ignored (the target re-renders what is missing).
fn carry_thumbnails(target: &LibraryHandle, package: &TransferPackage, committed: &[usize], read: ReadFile) {
    for &index in committed {
        for thumb in &package.assets[index].thumbs {
            let Ok(mut source) = read(&thumb.file.path) else { continue };
            let mut bytes = Vec::new();
            if source.read_to_end(&mut bytes).is_err() || sha_hex(&bytes) != thumb.file.sha256 {
                continue;
            }
            let _ = fsio::write_bytes_atomic(&target.thumb_path(&thumb.id), &bytes);
        }
    }
}

/// Favourites, tags, collections and provenance of an item that was new to the
/// target. An item that replaced one keeps the target's own. Only the rows of
/// these items are read and written.
fn carry_personal_metadata(target: &LibraryHandle, package: &TransferPackage, committed: &[usize], items: &[TransferItem]) -> AppResult<()> {
    let fresh: Vec<&PackageAsset> = committed.iter().filter(|&&index| items[index].outcome == Outcome::Copied).map(|&index| &package.assets[index]).collect();
    if fresh.iter().all(|asset| asset.user.is_empty()) {
        return Ok(());
    }
    super::catalog::open_library(target)?;
    let store = Store::for_handle(target);
    if !store.catalog_exists() {
        return Ok(());
    }
    // A preview choice may name any item of the target.
    let named: Vec<String> = fresh
        .iter()
        .flat_map(|asset| asset.user.values())
        .filter_map(|user| user["previewAssetId"].as_str().map(String::from))
        .collect();
    let known: HashSet<String> = store.id_owners(&named)?.into_keys().collect();
    let ids: Vec<String> = fresh.iter().map(|asset| asset.id.clone()).collect();
    let mut assets = store.load_top_assets(&ids)?;
    let mut puts = Vec::new();
    for asset in assets.iter_mut() {
        if let Some(source) = fresh.iter().find(|source| source.id == asset.id) {
            carry_user(asset, &source.user, &known);
            puts.push(ItemPut { id: asset.id.clone(), entry: None, assets: vec![asset.clone()], rigs: Vec::new() });
        }
    }
    store.apply(&Delta { puts, ..Default::default() })?;
    Ok(())
}

fn carry_user(asset: &mut Asset, user: &BTreeMap<String, Value>, known: &HashSet<String>) {
    if let Some(mut incoming) = user.get(&asset.id).and_then(|value| serde_json::from_value::<UserMeta>(value.clone()).ok()) {
        // A preview choice names another item of the source library; keep it only
        // when that item exists in the target too (or is one of the built-ins).
        if incoming.preview_asset_id.as_ref().is_some_and(|id| !id.starts_with("toybox:") && !known.contains(id)) {
            incoming.preview_asset_id = None;
        }
        if incoming.provenance.is_none() {
            incoming.provenance = asset.user.provenance.take();
        }
        for tag in std::mem::take(&mut asset.user.tags) {
            if !incoming.tags.contains(&tag) {
                incoming.tags.push(tag);
            }
        }
        asset.user = incoming;
    }
    for variant in &mut asset.variants {
        carry_user(variant, user, known);
    }
}

// --- Local to local -----------------------------------------------------------------

/// Copy or move `ids` of `source` into `target`, both on this machine.
///
/// A move first copies; then, for each item that was copied or replaced and is
/// verifiably in the target (registered, every file present), deletes it from
/// the source through the normal delete path. Nothing else is deleted: skipped
/// and failed items stay where they are, and an item whose copy failed is never
/// removed.
pub fn transfer_between(
    source: &LibraryHandle,
    target: &LibraryHandle,
    ids: &[String],
    options: TransferOptions,
    progress: &dyn Fn(TransferProgress),
) -> AppResult<TransferReport> {
    if source.id == target.id || crate::infra::registry::same_root(&source.root_str(), &target.root_str()) {
        return Err(AppError::msg("Choose a different library to transfer into"));
    }
    if !library_rules::is_writable(&target.root) {
        return Err(AppError::msg("The target library is read-only"));
    }
    if options.mode == TransferMode::Move && !library_rules::is_writable(&source.root) {
        return Err(AppError::msg("The source library is read-only, so items cannot be moved out of it. Copy them instead"));
    }
    let package = export_package(source, ids, ExportOptions { include_clips: options.include_clips })?;
    let reader = local_reader(source);
    let mut report = ingest_package(target, &package, IngestOptions { on_conflict: options.on_conflict }, &reader, progress)?;

    if options.mode == TransferMode::Move {
        let store = Store::for_handle(target);
        complete_move(
            &mut report,
            &|id| verify_present(target, &store, id),
            &|ids| super::delete::delete_from_library(&source.root, &source.cache_path, ids).map(|_| ()),
        );
    }
    sort_report(&mut report, ids);
    Ok(report)
}

/// Report in the order the items were asked for.
pub fn sort_report(report: &mut TransferReport, ids: &[String]) {
    let order: HashMap<&str, usize> = ids.iter().enumerate().rev().map(|(index, id)| (id.as_str(), index)).collect();
    report.items.sort_by_key(|item| order.get(item.id.as_str()).copied().unwrap_or(usize::MAX));
}

/// The delete half of a move: for each item that was copied or replaced and
/// `verify` confirms is really in the target, `delete` it from the source. A
/// batch that cannot be deleted is retried one by one, so an item that cannot be
/// removed does not keep the others in the source. Skipped and failed items are
/// never touched. Local libraries and remote ones share this.
pub fn complete_move(report: &mut TransferReport, verify: &dyn Fn(&str) -> AppResult<()>, delete: &dyn Fn(&[String]) -> AppResult<()>) {
    let mut movable: Vec<usize> = Vec::new();
    for (index, item) in report.items.iter_mut().enumerate() {
        if !matches!(item.outcome, Outcome::Copied | Outcome::Replaced) {
            continue;
        }
        match verify(&item.id) {
            Ok(()) => movable.push(index),
            Err(error) => item.reason = Some(format!("Copied, but not removed from the source: {error}")),
        }
    }
    let ids: Vec<String> = movable.iter().map(|&index| report.items[index].id.clone()).collect();
    if ids.is_empty() {
        return;
    }
    if delete(&ids).is_ok() {
        for &index in &movable {
            report.items[index].source_removed = true;
        }
        return;
    }
    for &index in &movable {
        let id = vec![report.items[index].id.clone()];
        match delete(&id) {
            Ok(()) => report.items[index].source_removed = true,
            Err(error) => report.items[index].reason = Some(format!("Copied, but not removed from the source: {error}")),
        }
    }
}

/// Whether the target library of a local copy registers `id` with every file present.
pub fn verify_item_present(target: &LibraryHandle, id: &str) -> AppResult<()> {
    verify_present(target, &Store::for_handle(target), id)
}

/// Fill in the personal metadata (`user`) of a package from a library cache on
/// this machine. Used for a remote source, whose server holds none.
pub fn fill_user_from_cache(package: &mut TransferPackage, cache: &Path) {
    let ids: Vec<String> = package.assets.iter().map(|asset| asset.id.clone()).collect();
    let cached: HashMap<String, Asset> = Store::remote_at(Path::new(""), cache).load_top_assets(&ids).unwrap_or_default().into_iter().map(|asset| (asset.id.clone(), asset)).collect();
    for asset in &mut package.assets {
        asset.user.clear();
        if let Some(found) = cached.get(&asset.id) {
            asset.user = user_of(found);
        }
    }
}

/// Carry the personal metadata of the items a report says were newly copied into
/// `target`'s cache (a replaced item keeps the target's own). The target's
/// catalog must already contain the items. Used when the target is a remote
/// library, whose server keeps no personal metadata.
pub fn apply_personal_metadata(target: &LibraryHandle, package: &TransferPackage, report: &TransferReport) -> AppResult<()> {
    let items: Vec<TransferItem> = package.assets.iter().map(|asset| {
        let outcome = report.items.iter().find(|item| item.id == asset.id).map(|item| item.outcome).unwrap_or(Outcome::Failed);
        TransferItem { id: asset.id.clone(), name: asset.name.clone(), kind: asset.kind.clone(), outcome, reason: None, source_removed: false }
    }).collect();
    let committed: Vec<usize> = items.iter().enumerate().filter(|(_, item)| matches!(item.outcome, Outcome::Copied | Outcome::Replaced)).map(|(index, _)| index).collect();
    carry_personal_metadata(target, package, &committed, &items)
}

/// The target manifest registers `id` and every file of it and of its variants exists.
fn verify_present(target: &LibraryHandle, store: &Store, id: &str) -> AppResult<()> {
    let entry = store.load_entries(&[id.to_string()])?.into_iter().next()
        .ok_or_else(|| AppError::msg("it is not registered in the target library"))?;
    let parsed = seed::parse_entry(&entry)?;
    for item in std::iter::once(&parsed).chain(&parsed.variants) {
        for path in item_paths(&target.root, item)? {
            if !fsio::exists(&paths::abs_under_root(&target.root_str(), &path)) {
                return Err(AppError::msg(format!("{path} is missing in the target library")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "transfer_tests.rs"]
mod tests;
