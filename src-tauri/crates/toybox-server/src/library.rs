//! The served library: its counters, the revision feed, and every write,
//! serialised through one async mutex. All the real work is `toybox-core`
//! (`ops::scan`, `ops::import`, `ops::delete`, the store's queries); this module
//! only schedules it on blocking threads, strips what is personal, and keeps the
//! books of what clients compare. It holds no copy of the library: there is no
//! snapshot to rebuild after a write.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::http::StatusCode;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use toybox_core::config as core_config;
use toybox_core::domain::catalog_model::{Asset, Catalog, ThumbMeta, UserMeta};
use toybox_core::domain::seed;
use toybox_core::error::AppError;
use toybox_core::infra::registry::{self, LibraryHandle};
use toybox_core::infra::store::{ChangesPage, ChangesQuery, Status, Store};
use toybox_core::ops::content_hash::{self, BackfillOptions};
use toybox_core::ops::files::{self, RelPathError};
use toybox_core::ops::metadata::MetadataEdit;
use toybox_core::ops::transfer::{ExportOptions, IngestOptions, OnConflict, Outcome, TransferItem, TransferPackage, TransferReport};

use crate::config::hex;
use crate::data::DataDir;
use crate::error::ApiError;

/// What says the library changed under what the server has checked: the library
/// database's identity and revision (every manifest or catalog write moves the
/// revision; a timestamp could not tell two quick writes apart).
#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    db: String,
    revision: i64,
}

/// The stamp of the library as it is now; `None` while it has no manifest.
fn stamp_of(handle: &LibraryHandle) -> Option<Stamp> {
    if !toybox_core::infra::store::manifest_exists(&handle.root) {
        return None;
    }
    let status = Store::for_handle(handle).status().ok()?;
    Some(Stamp { db: status.db_id, revision: status.revision })
}

/// The counters a client compares, as they are now (one read of the database).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub db_id: String,
    /// The library revision: `GET /changes` is keyed by it.
    pub revision: i64,
    pub hash_revision: i64,
    pub thumb_revision: i64,
}

impl From<Status> for State {
    fn from(status: Status) -> State {
        State { db_id: status.db_id, revision: status.revision, hash_revision: status.hash_revision, thumb_revision: status.thumb_revision }
    }
}

impl State {
    /// The revision as API version 1 clients know it: a short string that changes
    /// with the library content or its fingerprints. It is also the ETag of the
    /// whole catalog, so a change of either invalidates a client's copy.
    pub fn legacy_revision(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.db_id.as_bytes());
        hasher.update(core_config::SCHEMA_VERSION.to_le_bytes());
        hasher.update(self.revision.to_le_bytes());
        hasher.update(b"content-hashes");
        hasher.update(self.hash_revision.to_le_bytes());
        hex(&hasher.finalize()[..8])
    }
}

/// The whole catalog as API version 1 serves it. Built on demand, kept for the
/// revision it was built at, and never rebuilt as a side effect of a write.
pub struct LegacyCatalog {
    pub revision: String,
    /// `revision` as a quoted HTTP entity tag.
    pub etag: String,
    /// The catalog JSON without personal metadata.
    pub body: Bytes,
}

/// Where a write started and ended: the revisions around everything it did,
/// found while the library was locked, so `(base, revision]` holds its changes
/// and whatever ran beside it (fingerprints arriving).
#[derive(Debug, Clone, Copy)]
pub struct WriteInfo {
    pub base: i64,
    pub revision: i64,
}

/// What a client may learn about one asset: the asset with favourites, tags the
/// user typed, collections and preview choices removed. Tags that come from
/// import provenance (source, publisher, licence) stay, and the path the
/// importer read the files from is dropped.
pub fn make_public_asset(asset: &mut Asset) {
    let provenance = asset.user.provenance.take().map(|mut p| {
        p.filesystem_path = None;
        p
    });
    let tags = provenance.as_ref().map(|p| p.tags()).unwrap_or_default();
    asset.user = UserMeta { provenance, preview_asset_id: None, favorite: false, tags, collections: Vec::new() };
    // Whether a picture exists is answered by /thumbs, not by this file.
    asset.thumb = ThumbMeta::default();
    for variant in &mut asset.variants {
        make_public_asset(variant);
    }
}

/// [`make_public_asset`] for a catalog.
pub fn make_public(catalog: &mut Catalog) {
    catalog.library_root = String::new();
    for asset in &mut catalog.assets {
        make_public_asset(asset);
    }
}

/// The saved catalog when it was made from the manifest as it is now, otherwise
/// a rescan (which keeps what the cache already knows). Only the API version 1
/// whole-catalog answer reads the library this way.
fn load_or_scan(handle: &LibraryHandle) -> Result<Catalog, AppError> {
    // Finish any migration or interrupted publication first; they change what is stored.
    toybox_core::infra::migrations::run(&handle.root, &handle.cache_path)?;
    let cache_fresh = Store::for_handle(handle).catalog_is_current().unwrap_or(false);
    if cache_fresh {
        if let Some(catalog) = toybox_core::ops::catalog::load_current_catalog(handle)? {
            if registry::same_root(&catalog.library_root, &handle.root_str()) {
                return Ok(catalog);
            }
        }
    }
    toybox_core::ops::scan::scan_library_catalog(handle)
}

/// Hides the server's folder names from messages that reach clients.
#[derive(Clone)]
pub struct Scrubber(Arc<Vec<String>>);

impl Scrubber {
    pub fn new(data: &DataDir) -> Scrubber {
        let mut names = Vec::new();
        let mut add = |p: &Path| {
            let text = p.display().to_string();
            for variant in [text.clone(), text.replace('\\', "/")] {
                if variant.len() > 3 && !names.contains(&variant) {
                    names.push(variant);
                }
            }
        };
        add(&data.root);
        if let Ok(real) = data.root.canonicalize() {
            add(&real);
            // Windows canonical paths gain a `\\?\` prefix that the plain path does not have.
            if let Some(stripped) = real.display().to_string().strip_prefix(r"\\?\") {
                add(Path::new(stripped));
            }
        }
        names.sort_by_key(|n| std::cmp::Reverse(n.len()));
        Scrubber(Arc::new(names))
    }

    pub fn clean(&self, message: &str) -> String {
        let mut out = message.to_string();
        for name in self.0.iter() {
            out = out.replace(name.as_str(), "<server>");
        }
        out
    }
}

/// The served library. Nothing about its content is held in memory: every answer
/// is read from the library database for the rows it needs, and every write goes
/// through one async mutex and `toybox-core`'s incremental operations.
pub struct Library {
    pub data: DataDir,
    pub handle: LibraryHandle,
    pub scrub: Scrubber,
    write_lock: tokio::sync::Mutex<()>,
    /// The state of the library at the last check that migrations were finished and
    /// the derived rows were current; a different stamp (the library was replaced
    /// or changed from outside) makes the next request check again.
    ready: Mutex<Option<Stamp>>,
    /// The API version 1 catalog, for the revision it was built at.
    legacy: Mutex<Option<Arc<LegacyCatalog>>>,
    /// Storage keys of items whose files could not be read; not retried until a restart.
    unhashable: Mutex<HashSet<String>>,
    /// Set when the library is dropped, so a fingerprint pass in a blocking thread stops.
    stop: Arc<AtomicBool>,
}

impl Drop for Library {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Items fingerprinted between two reads of the pending list.
const FINGERPRINT_CHUNK: usize = 1000;
/// Files hashed side by side during a pass.
const FINGERPRINT_THREADS: usize = 2;

/// What a fingerprint pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FingerprintReport {
    pub hashed: usize,
    pub failed: usize,
}

impl Library {
    pub fn new(data: DataDir) -> Library {
        let handle = data.handle();
        let scrub = Scrubber::new(&data);
        Library {
            data,
            handle,
            scrub,
            write_lock: tokio::sync::Mutex::new(()),
            ready: Mutex::new(None),
            legacy: Mutex::new(None),
            unhashable: Mutex::new(HashSet::new()),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    fn core_error(&self, error: AppError) -> ApiError {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "library_error",
            format!("The library could not be read: {}", self.scrub.clean(&error.to_string())),
        )
    }

    /// Run blocking work on a blocking thread.
    async fn blocking<T: Send + 'static>(&self, work: impl FnOnce(&LibraryHandle) -> Result<T, AppError> + Send + 'static) -> Result<T, ApiError> {
        let handle = self.handle.clone();
        tokio::task::spawn_blocking(move || work(&handle)).await.map_err(ApiError::internal)?.map_err(|e| self.core_error(e))
    }

    fn is_ready(&self) -> bool {
        let ready = self.ready.lock().unwrap_or_else(|p| p.into_inner());
        ready.as_ref().is_some_and(|stamp| Some(stamp) == stamp_of(&self.handle).as_ref())
    }

    fn mark_ready(&self) {
        *self.ready.lock().unwrap_or_else(|p| p.into_inner()) = stamp_of(&self.handle);
    }

    /// Make sure migrations and any interrupted publication are finished and the
    /// derived rows are current, once for each state the library is found in. A
    /// library that has them costs one read of two counters.
    pub async fn ensure_ready(&self) -> Result<(), ApiError> {
        if self.is_ready() {
            return Ok(());
        }
        let _guard = self.write_lock.lock().await;
        self.ready_locked().await
    }

    async fn ready_locked(&self) -> Result<(), ApiError> {
        if self.is_ready() {
            return Ok(());
        }
        self.blocking(|handle| toybox_core::ops::catalog::ready_for_write(handle)).await?;
        self.mark_ready();
        Ok(())
    }

    /// The counters of the library, after making it ready.
    pub async fn state(&self) -> Result<State, ApiError> {
        self.ensure_ready().await?;
        self.status().await
    }

    async fn status(&self) -> Result<State, ApiError> {
        self.blocking(|handle| Store::for_handle(handle).status().map(State::from)).await
    }

    /// One page of the revision feed, straight from the library database.
    pub async fn changes(&self, since: i64, after: Option<String>, limit: usize, db_id: Option<String>) -> Result<ChangesPage, ApiError> {
        self.ensure_ready().await?;
        let mut page = self
            .blocking(move |handle| Store::for_handle(handle).changes_page(&ChangesQuery { since, after: after.as_deref(), limit, db_id: db_id.as_deref() }))
            .await?;
        // The feed carries library data only; nothing personal can get in, and this makes sure.
        for asset in &mut page.assets {
            make_public_asset(asset);
        }
        Ok(page)
    }

    /// The whole catalog for clients of API version 1. Built when asked for and
    /// kept for the revision it was built at; writes do not build it.
    pub async fn legacy_catalog(&self) -> Result<Arc<LegacyCatalog>, ApiError> {
        self.ensure_ready().await?;
        let fresh = |library: &Library, state: &State| -> Option<Arc<LegacyCatalog>> {
            let held = library.legacy.lock().unwrap_or_else(|p| p.into_inner()).clone()?;
            (held.revision == state.legacy_revision()).then_some(held)
        };
        if let Some(held) = fresh(self, &self.status().await?) {
            return Ok(held);
        }
        // Built with writes held off, so the revision named is the content built.
        let _guard = self.write_lock.lock().await;
        let state = self.status().await?;
        if let Some(held) = fresh(self, &state) {
            return Ok(held);
        }
        let body = self
            .blocking(move |handle| {
                let mut catalog = load_or_scan(handle)?;
                content_hash::fill_from_cache(handle, &mut catalog);
                make_public(&mut catalog);
                Ok(serde_json::to_vec(&catalog)?)
            })
            .await?;
        // Building may have saved a catalog (a rescan), which moves the counters.
        let revision = self.status().await?.legacy_revision();
        let built = Arc::new(LegacyCatalog { etag: format!("\"{revision}\""), revision, body: Bytes::from(body) });
        *self.legacy.lock().unwrap_or_else(|p| p.into_inner()) = Some(built.clone());
        Ok(built)
    }

    /// Which of these ids name an asset or a variant of the library.
    pub async fn known_ids(&self, ids: Vec<String>) -> Result<HashSet<String>, ApiError> {
        self.ensure_ready().await?;
        self.blocking(move |handle| Store::for_handle(handle).known_ids(&ids)).await
    }

    /// Save a shared picture for an asset or variant of the library; `None` when
    /// the library has no such id. Records it in the thumbnail log.
    pub async fn save_thumb(&self, id: String, bytes: Vec<u8>) -> Result<Option<i64>, ApiError> {
        self.ensure_ready().await?;
        self.blocking(move |handle| {
            let store = Store::for_handle(handle);
            if store.known_ids(std::slice::from_ref(&id))?.is_empty() {
                return Ok(None);
            }
            toybox_core::ops::thumbs::save_thumb(handle, &id, &bytes)?;
            Ok(Some(store.note_thumbs(&[id])?))
        })
        .await
    }

    /// Ids a client may ask pictures for (every asset and variant).
    pub async fn all_ids(&self) -> Result<Vec<String>, ApiError> {
        self.ensure_ready().await?;
        self.blocking(|handle| Store::for_handle(handle).all_ids()).await
    }

    /// The ids whose picture is the file named by one of `names` (sanitised ids), in id order.
    pub async fn ids_by_thumb_name(&self, names: Vec<String>) -> Result<Vec<String>, ApiError> {
        self.ensure_ready().await?;
        self.blocking(move |handle| Store::for_handle(handle).ids_by_thumb_name(&names)).await
    }

    /// Ids whose picture was saved after thumbnail revision `since`, and the current one.
    pub async fn thumbs_since(&self, since: i64) -> Result<(Vec<String>, i64), ApiError> {
        self.ensure_ready().await?;
        self.blocking(move |handle| Store::for_handle(handle).thumbs_since(since)).await
    }

    /// Fingerprint the items that have none yet, in the background-friendly way:
    /// chunks of a thousand pending items read off the write lock, the hashes saved
    /// as they come (each save is a library revision that `/changes` carries to
    /// clients). Items whose files cannot be read are left out until the next start.
    pub async fn fingerprint_pass(&self) -> Result<FingerprintReport, ApiError> {
        let mut report = FingerprintReport::default();
        let mut after = String::new();
        let mut first = true;
        loop {
            self.ensure_ready().await?;
            let skip = self.unhashable.lock().unwrap_or_else(|p| p.into_inner()).clone();
            let stop = self.stop.clone();
            let (from, prune) = (after.clone(), std::mem::take(&mut first));
            let (chunk, next) = self
                .blocking(move |handle| {
                    // Entries for items long gone are dropped once per start, from the keys alone.
                    if prune {
                        content_hash::prune_hashes(handle)?;
                    }
                    let (catalog, next) = content_hash::unhashed_page(handle, &from, FINGERPRINT_CHUNK)?;
                    let options = BackfillOptions { skip: &skip, threads: FINGERPRINT_THREADS, ..Default::default() };
                    let chunk = content_hash::backfill(&handle.root, &content_hash::cache_path(handle), &catalog, &stop, &options, &mut |_| {})?;
                    Ok((chunk, next))
                })
                .await?;
            report.hashed += chunk.hashed;
            report.failed += chunk.failed.len();
            self.unhashable.lock().unwrap_or_else(|p| p.into_inner()).extend(chunk.failed.iter().cloned());
            match next {
                Some(last) if !chunk.cancelled => after = last,
                _ => break,
            }
        }
        Ok(report)
    }

    /// Run one write with the library locked. `work` runs on a blocking thread.
    /// Returns its result and the revisions before and after (see [`WriteInfo`]).
    pub async fn write<T, F>(&self, work: F) -> Result<(T, WriteInfo), ApiError>
    where
        T: Send + 'static,
        F: FnOnce(&LibraryHandle) -> Result<T, ApiError> + Send + 'static,
    {
        let _guard = self.write_lock.lock().await;
        // Taken before anything is made ready, so a rescan that does is part of what the write changed.
        let base = self.status().await?.revision;
        self.ready_locked().await?;
        let handle = self.handle.clone();
        let value = tokio::task::spawn_blocking(move || work(&handle)).await.map_err(ApiError::internal)??;
        self.mark_ready();
        let revision = self.status().await?.revision;
        Ok((value, WriteInfo { base, revision }))
    }

    /// What a write changed, for its reply: the feed after `info.base`. A write
    /// that changed more than `limit` rows gets a page that says `more`, and the
    /// client pulls the rest.
    pub async fn reply_changes(&self, info: WriteInfo, limit: usize) -> Result<ChangesPage, ApiError> {
        self.changes(info.base, None, limit, None).await
    }

    /// Remove top-level assets by id. Returns the ids deleted.
    pub async fn delete(&self, ids: Vec<String>) -> Result<(Vec<String>, WriteInfo), ApiError> {
        let scrub = self.scrub.clone();
        self.write(move |handle| {
            let present = Store::for_handle(handle).existing_asset_ids(&ids).map_err(|e| ApiError::internal(scrub.clean(&e.to_string())))?;
            let missing: Vec<&String> = ids.iter().filter(|id| !present.contains(*id)).collect();
            if !missing.is_empty() {
                return Err(ApiError::not_found(format!(
                    "{} of the selected items are not in the library any more (first: {}). Reload the catalog and try again.",
                    missing.len(),
                    missing[0]
                )));
            }
            toybox_core::ops::delete::delete_from_library(&handle.root, &handle.cache_path, &ids).map_err(|e| {
                ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "delete_failed", scrub.clean(&e.to_string()))
            })?;
            Ok(ids)
        })
        .await
    }

    /// Change the library tags and provenance of items. Returns the ids of the
    /// top-level items the edit touched (an edited variant counts for its parent).
    pub async fn update_metadata(&self, edits: Vec<MetadataEdit>) -> Result<(Vec<String>, WriteInfo), ApiError> {
        let scrub = self.scrub.clone();
        self.write(move |handle| {
            let store = Store::for_handle(handle);
            let asked: Vec<String> = edits.iter().map(|edit| edit.id.clone()).collect();
            let known = store.known_ids(&asked).map_err(|e| ApiError::internal(scrub.clean(&e.to_string())))?;
            let missing: Vec<&str> = asked.iter().map(String::as_str).filter(|id| !known.contains(*id)).collect();
            if let Some(first) = missing.first() {
                return Err(ApiError::not_found(format!(
                    "{} of the selected items are not in the library any more (first: {first}). Reload the catalog and try again.",
                    missing.len()
                )));
            }
            toybox_core::ops::metadata::update_entries(handle, &edits).map_err(|e| {
                ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "metadata_failed", scrub.clean(&e.to_string()))
            })?;
            store.owner_asset_ids(&asked).map_err(|e| ApiError::internal(scrub.clean(&e.to_string())))
        })
        .await
    }

    /// The top-level assets with these ids as the server serves them.
    pub async fn served_assets(&self, ids: Vec<String>) -> Result<Vec<Asset>, ApiError> {
        self.ensure_ready().await?;
        let mut assets = self.blocking(move |handle| Store::for_handle(handle).served_assets(&ids)).await?;
        for asset in &mut assets {
            make_public_asset(asset);
        }
        Ok(assets)
    }

    /// Describe items as a transfer package, with server folder names removed
    /// from messages and no personal metadata.
    pub async fn export_package(&self, ids: Vec<String>, include_clips: bool) -> Result<TransferPackage, ApiError> {
        self.ensure_ready().await?;
        let scrub = self.scrub.clone();
        let handle = self.handle.clone();
        let mut package = tokio::task::spawn_blocking(move || {
            toybox_core::ops::transfer::export_package(&handle, &ids, ExportOptions { include_clips })
        })
        .await
        .map_err(ApiError::internal)?
        .map_err(|e| self.core_error(e))?;
        for asset in &mut package.assets {
            asset.user.clear();
        }
        for failure in &mut package.failed {
            failure.reason = scrub.clean(&failure.reason);
        }
        package.source_library = String::new();
        Ok(package)
    }

    /// Run core's ingest over a package whose files sit in `package_dir`.
    pub async fn ingest(&self, package_dir: PathBuf, package: TransferPackage, on_conflict: OnConflict) -> Result<(TransferReport, WriteInfo), ApiError> {
        let scrub = self.scrub.clone();
        self.write(move |handle| ingest_blocking(handle, &package_dir, package, on_conflict, &scrub)).await
    }

    /// Place the files of an upload where core expects staged files, merge the
    /// entries, and report per entry. See [`commit_blocking`].
    pub async fn commit(&self, staging_files: PathBuf, entries: Vec<Value>) -> Result<(CommitReport, WriteInfo), ApiError> {
        let scrub = self.scrub.clone();
        self.write(move |handle| commit_blocking(handle, &staging_files, entries, &scrub)).await
    }
}

/// The stored kept source (`library/<type>/_sources/<sha256>/<name>`) with this content, if the library has one.
pub fn stored_source(root: &Path, sha256: &str) -> Option<PathBuf> {
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    for folder in ["materials", "models"] {
        let dir = root.join("library").join(folder).join(toybox_core::domain::sources::SOURCES_DIR).join(sha256);
        if let Ok(read) = std::fs::read_dir(dir) {
            if let Some(file) = read.flatten().find(|entry| entry.file_type().is_ok_and(|kind| kind.is_file())) {
                return Some(file.path());
            }
        }
    }
    None
}

/// Where a linked file comes from.
pub enum LinkSource {
    /// The library's own stored source with this digest.
    Library(String),
    /// Another file of the same upload (path relative to the upload's files folder).
    Upload(String),
}

/// Hard-link `source` to `target` (a copy where links are not possible).
fn link_file(source: &Path, target: &Path) -> Result<(), ApiError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(ApiError::internal)?;
    }
    if target.exists() {
        return Ok(());
    }
    if std::fs::hard_link(source, target).is_err() {
        std::fs::copy(source, target).map_err(ApiError::internal)?;
    }
    Ok(())
}

impl Library {
    /// Which of these digests the library stores as kept sources already.
    pub async fn have_sources(&self, hashes: Vec<String>) -> Result<Vec<String>, ApiError> {
        self.blocking(move |handle| Ok(hashes.into_iter().filter(|hash| stored_source(&handle.root, hash).is_some()).collect())).await
    }

    /// Put files into an upload without sending them: each `(path in the upload, source)`.
    pub async fn link_into_upload(&self, base: PathBuf, links: Vec<(String, LinkSource)>) -> Result<(), ApiError> {
        let handle = self.handle.clone();
        tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
            for (rel, source) in links {
                let target = files::resolve_for_write(&base, &rel).map_err(|_| ApiError::bad_request("bad_upload_path", "That path is not allowed."))?;
                let from = match source {
                    LinkSource::Library(hash) => stored_source(&handle.root, &hash).ok_or_else(|| ApiError::conflict("source_missing", "The server does not store that source file (any more). Upload it."))?,
                    LinkSource::Upload(other) => {
                        let path = files::resolve_for_write(&base, &other).map_err(|_| ApiError::bad_request("bad_upload_path", "That path is not allowed."))?;
                        if !path.is_file() {
                            return Err(ApiError::conflict("source_missing", "A file to link from was not uploaded."));
                        }
                        path
                    }
                };
                link_file(&from, &target)?;
            }
            Ok(())
        })
        .await
        .map_err(ApiError::internal)?
    }
}

#[derive(Debug, Serialize)]
pub struct EntryError {
    pub index: usize,
    pub id: Option<String>,
    pub message: String,
}

#[derive(Debug, Default, Serialize)]
pub struct CommitReport {
    pub imported: Vec<String>,
    pub errors: Vec<EntryError>,
}

pub const MAX_ENTRIES: usize = 5000;

fn safe_segment(text: &str) -> bool {
    !text.is_empty()
        && text != "."
        && text != ".."
        && !text.contains(['/', '\\'])
        && !text.chars().any(char::is_control)
}

/// A relative path with `/` separators: no empty, `.` or `..` parts, no
/// backslashes, no drive letters.
fn safe_rel(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 1024
        && !text.contains('\\')
        && !text.chars().any(char::is_control)
        && text.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
        && !text.split('/').next().is_some_and(|first| first.len() == 2 && first.ends_with(':'))
}

fn valid_pack(pack: &str) -> bool {
    pack.starts_with("_import_")
        && pack.len() > 8
        && pack.len() <= 64
        && pack.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn check_item(item: &Value) -> Result<(), String> {
    let Some(object) = item.as_object() else { return Err("Each entry must be an object.".into()) };
    if object.contains_key("storagePath") {
        return Err("Entries may not set storagePath; the server chooses where files are stored.".into());
    }
    match object.get("pack").and_then(Value::as_str) {
        Some(pack) if valid_pack(pack) => {}
        _ => return Err("An entry's pack must be the staging name of the upload, like _import_1a2b3c.".into()),
    }
    if let Some(name) = object.get("name") {
        if !name.as_str().is_some_and(safe_segment) {
            return Err("An entry's name must be plain text without slashes.".into());
        }
    }
    let path_fields = ["category", "file"];
    for key in path_fields {
        if let Some(value) = object.get(key) {
            if !value.as_str().is_some_and(safe_rel) {
                return Err(format!("The \"{key}\" of an entry must be a plain relative path."));
            }
        }
    }
    let list_ok = |value: Option<&Value>| -> bool {
        match value {
            None | Some(Value::Null) => true,
            Some(Value::Array(items)) => items.iter().all(|v| v.as_str().is_some_and(safe_rel)),
            Some(_) => false,
        }
    };
    if !list_ok(object.get("files")) || !list_ok(object.get("origin").and_then(|o| o.get("files"))) {
        return Err("File lists must contain plain relative paths.".into());
    }
    if let Some(maps) = object.get("maps") {
        let ok = maps.as_array().is_some_and(|maps| maps.iter().all(|m| m.get("file").and_then(Value::as_str).is_some_and(safe_rel)));
        if !ok {
            return Err("Every material map needs a plain relative file name.".into());
        }
    }
    Ok(())
}

/// Server-side checks on an entry before core sees it. Returns the entry id.
pub fn check_entry(entry: &Value) -> Result<String, String> {
    check_item(entry)?;
    for variant in entry.get("variants").and_then(Value::as_array).into_iter().flatten() {
        check_item(variant)?;
    }
    entry
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 512)
        .map(String::from)
        .ok_or_else(|| "Every entry needs an id.".to_string())
}

/// Make the staged files of `packs` appear under the library root, where
/// `merge_entries` looks for them (`library/<pack>/...`). Files are hard-linked
/// when the folders share a disk, copied otherwise, so an upload costs no extra
/// space until core copies it into managed storage. Idempotent.
fn place(staging_files: &Path, root: &Path, packs: &BTreeSet<String>) -> Result<(), ApiError> {
    let fail = |what: &str, e: &dyn std::fmt::Display| ApiError::internal(format!("placing staged files: {what}: {e}"));
    for pack in packs {
        let source = staging_files.join("library").join(pack);
        if !source.is_dir() {
            continue;
        }
        let mut pending = vec![source];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).map_err(|e| fail("read staging", &e))? {
                let entry = entry.map_err(|e| fail("read staging", &e))?;
                let kind = entry.file_type().map_err(|e| fail("stat staging", &e))?;
                let path = entry.path();
                if kind.is_dir() {
                    pending.push(path);
                    continue;
                }
                if !kind.is_file() || path.extension().is_some_and(|x| x == "tbx-part") {
                    continue;
                }
                let rel = path
                    .strip_prefix(staging_files)
                    .map_err(|e| fail("relative path", &e))?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                let target = files::resolve_for_write(root, &rel).map_err(|e| fail("target", &e))?;
                if target.exists() {
                    continue;
                }
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| fail("create folder", &e))?;
                }
                if std::fs::hard_link(&path, &target).is_err() {
                    std::fs::copy(&path, &target).map_err(|e| fail("copy", &e))?;
                }
            }
        }
    }
    Ok(())
}

fn entry_pack(entry: &Value) -> String {
    entry.get("pack").and_then(Value::as_str).unwrap_or_default().to_string()
}

/// The blocking half of an upload commit. Entries are checked one by one;
/// valid ones are merged together (core validates everything before it writes
/// anything). If that fails, they are retried one at a time so one bad entry
/// does not sink the rest. Staged files are placed under `library/_import_*`
/// for the duration and removed afterwards; the upload folder is never changed.
pub fn commit_blocking(
    handle: &LibraryHandle,
    staging_files: &Path,
    entries: Vec<Value>,
    scrub: &Scrubber,
) -> Result<CommitReport, ApiError> {
    let mut report = CommitReport::default();
    let mut valid: Vec<(String, Value)> = Vec::new();
    let mut seen = HashSet::new();
    for (index, entry) in entries.into_iter().enumerate() {
        let id_text = entry.get("id").and_then(Value::as_str).map(String::from);
        let checked = check_entry(&entry).and_then(|id| {
            if !seen.insert(id.clone()) {
                return Err(format!("The id {id} appears twice in this import."));
            }
            seed::parse_entry(&entry).map(|_| id).map_err(|e| scrub.clean(&e.to_string()))
        });
        match checked {
            Ok(id) => valid.push((id, entry)),
            Err(message) => report.errors.push(EntryError { index, id: id_text, message }),
        }
    }
    if valid.is_empty() {
        return Ok(report);
    }
    let packs: BTreeSet<String> = valid.iter().map(|(_, e)| entry_pack(e)).collect();
    let clean_staging = |packs: &BTreeSet<String>| {
        for pack in packs {
            let _ = std::fs::remove_dir_all(handle.root.join("library").join(pack));
        }
    };
    // Anything left in these folders is debris from an earlier crash.
    clean_staging(&packs);
    let outcome = (|| -> Result<(), ApiError> {
        place(staging_files, &handle.root, &packs)?;
        let all: Vec<Value> = valid.iter().map(|(_, e)| e.clone()).collect();
        match toybox_core::ops::import::merge_entries(handle, all) {
            Ok(_) => {
                report.imported = valid.iter().map(|(id, _)| id.clone()).collect();
            }
            Err(batch_error) if valid.len() == 1 => {
                report.errors.push(EntryError { index: 0, id: Some(valid[0].0.clone()), message: scrub.clean(&batch_error.to_string()) });
            }
            Err(_) => {
                for (id, entry) in &valid {
                    place(staging_files, &handle.root, &packs)?;
                    match toybox_core::ops::import::merge_entries(handle, vec![entry.clone()]) {
                        Ok(_) => report.imported.push(id.clone()),
                        Err(error) => report.errors.push(EntryError { index: 0, id: Some(id.clone()), message: scrub.clean(&error.to_string()) }),
                    }
                }
            }
        }
        Ok(())
    })();
    clean_staging(&packs);
    outcome.map(|()| report)
}

/// Server-side checks on one entry of a transfer package before core stages it.
/// Package entries are manifest entries as another library records them, so
/// they carry a `storagePath` (core drops it) and any pack, but every name that
/// becomes a folder or file name during staging must be plain.
pub fn check_package_entry(entry: &Value) -> Result<String, String> {
    fn item(item: &Value) -> Result<(), String> {
        let Some(object) = item.as_object() else { return Err("Each entry must be an object.".into()) };
        for key in ["name", "type"] {
            if let Some(value) = object.get(key) {
                if !value.as_str().is_some_and(safe_segment) {
                    return Err(format!("The \"{key}\" of an entry must be plain text without slashes."));
                }
            }
        }
        for key in ["pack", "category", "file"] {
            if let Some(value) = object.get(key) {
                if !value.as_str().is_some_and(safe_rel) {
                    return Err(format!("The \"{key}\" of an entry must be a plain relative path."));
                }
            }
        }
        let list_ok = |value: Option<&Value>| match value {
            None | Some(Value::Null) => true,
            Some(Value::Array(items)) => items.iter().all(|v| v.as_str().is_some_and(safe_rel)),
            Some(_) => false,
        };
        if !list_ok(object.get("files")) || !list_ok(object.get("origin").and_then(|o| o.get("files"))) {
            return Err("File lists must contain plain relative paths.".into());
        }
        if let Some(maps) = object.get("maps") {
            let ok = maps.as_array().is_some_and(|maps| maps.iter().all(|m| m.get("file").and_then(Value::as_str).is_some_and(safe_rel)));
            if !ok {
                return Err("Every material map needs a plain relative file name.".into());
            }
        }
        Ok(())
    }
    item(entry)?;
    for variant in entry.get("variants").and_then(Value::as_array).into_iter().flatten() {
        item(variant)?;
    }
    entry
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 512)
        .map(String::from)
        .ok_or_else(|| "Every entry needs an id.".to_string())
}

/// A package path the server will read: a library file or a thumbnail.
fn package_path_ok(path: &str) -> bool {
    safe_rel(path) && (path.starts_with("library/") || (path.starts_with("thumbs/") && path.matches('/').count() == 1 && path.ends_with(".png")))
}

/// The blocking half of an ingest. Entries that fail the server checks are
/// reported as failed without reaching core; the rest are ingested by
/// `toybox_core::ops::transfer::ingest_package`.
pub fn ingest_blocking(
    handle: &LibraryHandle,
    package_dir: &Path,
    mut package: TransferPackage,
    on_conflict: OnConflict,
    scrub: &Scrubber,
) -> Result<TransferReport, ApiError> {
    use std::io::Read;
    let mut refused: Vec<TransferItem> = Vec::new();
    let mut kept = Vec::new();
    for mut asset in std::mem::take(&mut package.assets) {
        // Favourites and tags belong to the client that owns the cache.
        asset.user.clear();
        let bad_path = asset.files.iter().map(|f| &f.path).chain(asset.thumbs.iter().map(|t| &t.file.path)).find(|p| !package_path_ok(p)).cloned();
        let verdict = match (check_package_entry(&asset.entry), bad_path) {
            (Err(message), _) => Err(message),
            (_, Some(path)) => Err(format!("The package names a path that is not allowed: {path}")),
            (Ok(_), None) => Ok(()),
        };
        match verdict {
            Ok(()) => kept.push(asset),
            Err(reason) => refused.push(TransferItem { id: asset.id, name: asset.name, kind: asset.kind, outcome: Outcome::Failed, reason: Some(reason), source_removed: false }),
        }
    }
    package.assets = kept;
    let root = package_dir.to_path_buf();
    let read = move |path: &str| -> Result<Box<dyn Read>, AppError> {
        if !package_path_ok(path) {
            return Err(AppError::msg(format!("Not a package path: {path}")));
        }
        let real = files::resolve_existing(&root, path).map_err(|_| AppError::msg(format!("The file was not uploaded: {path}")))?;
        if path.starts_with("thumbs/") {
            let bytes = std::fs::read(&real)?;
            if bytes.len() < 8 || bytes[..8] != [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] || bytes.len() > 2 * 1024 * 1024 {
                return Err(AppError::msg("A thumbnail must be a PNG image of at most 2 MB"));
            }
            return Ok(Box::new(std::io::Cursor::new(bytes)));
        }
        Ok(Box::new(std::fs::File::open(&real)?))
    };
    let mut report = toybox_core::ops::transfer::ingest_package(handle, &package, IngestOptions { on_conflict }, &read, &|_| {})
        .map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "import_failed", scrub.clean(&e.to_string())))?;
    for item in &mut report.items {
        if let Some(reason) = &mut item.reason {
            *reason = scrub.clean(reason);
        }
    }
    report.items.extend(refused);
    Ok(report)
}

pub fn path_error(error: RelPathError) -> ApiError {
    match error {
        RelPathError::NotFound | RelPathError::NotAFile => ApiError::not_found("There is no such file."),
        RelPathError::Escapes => ApiError::not_found("There is no such file."),
        other => ApiError::bad_request("bad_path", format!("That file path is not allowed: {other}.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn entries_with_unsafe_fields_are_refused() {
        let ok = json!({"id":"a","kind":"plugin","type":"skybox","pack":"_import_ab12","category":"day","name":"Noon","files":["noon.hdr"]});
        assert_eq!(check_entry(&ok).unwrap(), "a");
        let bad = |patch: Value| {
            let mut entry = ok.clone();
            for (k, v) in patch.as_object().unwrap() {
                entry[k] = v.clone();
            }
            check_entry(&entry)
        };
        assert!(bad(json!({"pack":"library"})).is_err());
        assert!(bad(json!({"pack":"_import_"})).is_err());
        assert!(bad(json!({"pack":"_import_../x"})).is_err());
        assert!(bad(json!({"name":"../x"})).is_err());
        assert!(bad(json!({"name":"a/b"})).is_err());
        assert!(bad(json!({"category":"../../x"})).is_err());
        assert!(bad(json!({"files":["../x.hdr"]})).is_err());
        assert!(bad(json!({"files":["C:/x.hdr"]})).is_err());
        assert!(bad(json!({"files":["a\\b.hdr"]})).is_err());
        assert!(bad(json!({"files":"x"})).is_err());
        assert!(bad(json!({"storagePath":"library/skybox/0123456789abcdef"})).is_err());
        assert!(bad(json!({"id":""})).is_err());
        assert!(bad(json!({"origin":{"plugin":"p","files":["../x"]}})).is_err());
        assert!(bad(json!({"maps":[{"role":"albedo","file":"../x.png"}]})).is_err());
        assert!(bad(json!({"variants":[{"id":"v","pack":"elsewhere"}]})).is_err());
        assert!(check_entry(&json!("text")).is_err());
    }

    #[test]
    fn package_entries_keep_their_storage_path_and_pack_but_not_unsafe_names() {
        let ok = json!({"id":"a","kind":"plugin","type":"skybox","pack":"synty_city","category":"day","name":"Noon","storagePath":"library/skybox/0123456789abcdef","files":["noon.hdr"],
            "origin":{"plugin":"p","files":["noon.zip"]},"variants":[{"id":"b","pack":"synty_city","category":"day","name":"Noon2"}]});
        assert_eq!(check_package_entry(&ok).unwrap(), "a");
        let bad = |patch: Value| {
            let mut entry = ok.clone();
            for (k, v) in patch.as_object().unwrap() {
                entry[k] = v.clone();
            }
            check_package_entry(&entry)
        };
        assert!(bad(json!({"name":"../x"})).is_err());
        assert!(bad(json!({"name":"a/b"})).is_err());
        assert!(bad(json!({"type":"../skybox"})).is_err());
        assert!(bad(json!({"pack":"../x"})).is_err());
        assert!(bad(json!({"category":"C:/x"})).is_err());
        assert!(bad(json!({"files":["../x.hdr"]})).is_err());
        assert!(bad(json!({"origin":{"plugin":"p","files":["/abs"]}})).is_err());
        assert!(bad(json!({"maps":[{"role":"albedo","file":"../x.png"}]})).is_err());
        assert!(bad(json!({"variants":[{"id":"v","name":"../x"}]})).is_err());
        assert!(bad(json!({"id":""})).is_err());
        assert!(check_package_entry(&json!(["not an object"])).is_err());
    }

    #[test]
    fn only_library_files_and_thumbnails_can_be_package_paths() {
        for good in ["library/models/0123456789abcdef/a.gltf", "library/models/_resources/abc.png", "thumbs/uid___a.png"] {
            assert!(package_path_ok(good), "{good}");
        }
        for bad in ["_library_config/catalog.json", "library", "library/../x", "thumbs/a/b.png", "thumbs/a.txt", "other/a", "/library/a/b", "library\\a\\b", ""] {
            assert!(!package_path_ok(bad), "{bad}");
        }
    }

    #[test]
    fn public_catalog_drops_personal_metadata_but_keeps_provenance_tags() {
        use toybox_core::domain::provenance::ImportProvenance;
        let entry = json!({"id":"a","kind":"plugin","type":"skybox","pack":"p","category":"c","name":"Sky","files":["sky.hdr"],"tags":["Sky","noon"]});
        let mut asset = seed::build_plugin_asset(seed::parse_entry(&entry).unwrap()).unwrap();
        asset.user.favorite = true;
        asset.user.tags = vec!["mine".into(), "license: CC0".into()];
        asset.user.collections = vec!["Project".into()];
        asset.user.preview_asset_id = Some("b".into());
        asset.user.provenance = Some(ImportProvenance {
            filesystem_path: Some("D:\\downloads\\pack.zip".into()),
            license: Some("CC0".into()),
            ..Default::default()
        });
        let mut catalog = Catalog { rigs: vec![], schema_version: core_config::SCHEMA_VERSION, library_root: "/srv/secret".into(), assets: vec![asset] };
        make_public(&mut catalog);
        let text = serde_json::to_string(&catalog).unwrap();
        assert!(!text.contains("/srv/secret") && !text.contains("downloads") && !text.contains("mine"));
        let user = &catalog.assets[0].user;
        assert!(!user.favorite && user.collections.is_empty() && user.preview_asset_id.is_none());
        assert_eq!(user.tags, vec!["license: CC0"]);
        // Library tags are the library's, not personal: they are served.
        assert_eq!(catalog.assets[0].tags, vec!["sky", "noon"]);
        assert!(text.contains(r#""tags":["sky","noon"]"#));
        assert_eq!(user.provenance.as_ref().unwrap().license.as_deref(), Some("CC0"));
    }

    #[test]
    fn messages_do_not_reveal_the_data_folder() {
        let dir = std::env::temp_dir().join(format!("toybox-scrub-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let scrub = Scrubber::new(&DataDir::new(&dir));
        let message = format!("Read '{}': not found", dir.join("library").join("x.png").display());
        let cleaned = scrub.clean(&message);
        assert!(!cleaned.contains("toybox-scrub-test"), "{cleaned}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
