//! The content fingerprint of an asset: a SHA-256 over what the asset IS, not
//! over what it is called or where it lives. Two libraries holding byte-identical
//! files under different ids, names, packs, categories or file names produce the
//! same `contentHash`, which is how the app recognises a copy across libraries.
//!
//! # Definition (scheme version 1)
//!
//! The preimage always starts with `toybox-content-v1\n<type>\n`, where `<type>`
//! is the library type (`model`, `animation`, `material`, `audio` or a plugin's
//! type id), followed by `name=value\n` lines. A file is represented by `D(file)`,
//! the lower-case hex SHA-256 of its bytes.
//!
//! | type | lines |
//! | --- | --- |
//! | model, animation | `gltf=` SHA-256 of the *normalised* glTF JSON (below); for a clip that is not a standalone file also `clip=<index>` |
//! | material | one `map=<role>\|<channel or ->\|<invert 0/1>\|D(file)` per map, sorted, then `flipNormalGreen=<0/1>` |
//! | audio | `file=D(file)` |
//! | plugin-defined | one `file=D(file)` per entry of `files`, in order |
//!
//! Normalised glTF: the JSON is parsed; every `buffers[].uri` / `images[].uri` that
//! names a file is replaced by `sha256:D(file)`; embedded `data:` and remote URIs
//! stay as they are; the identity records Toybox itself writes into a stored scene
//! are removed (`extras.toyboxSourcePath`, `extensions.TOYBOX_library.sourcePath`
//! — the staging path of the import, which contains the file name — and
//! `extras.toyboxAnimation.id`, `.clip.sourceId`, `.clip.sourceName` of an
//! extracted clip — ids and names); then the document is written compactly. Nothing
//! else is touched. Kept originals (`origin.files`) and variants never take part
//! (a variant has a hash of its own).
//!
//! # Derived, never stale
//!
//! The hash is not part of any manifest. Stored files live in content-addressed
//! folders (`library/<type>/<key>/...`, the key changes whenever the id, the entry
//! or any file changes), so a side cache maps that `<type>/<key>` to the hash and
//! an entry that is in it is never read again. The side cache is the `hashes`
//! table of the library's `library.db` (`infra::store::hashes`; it was
//! `content-hashes.json` before library migration step 4), written in one
//! transaction per save; the table is stamped with the scheme, and another
//! `scheme` than the running one is ignored (so a change of definition starts
//! over). The "cache file" the functions below take is that database file
//! (`cache_path`).
//!
//! The one thing that edits a stored file in place is origin alignment
//! (`recenter_asset`); it forgets and recomputes the hash of the item it changed.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::catalog_model::{Asset, AssetContent, Catalog};
use crate::domain::gltf_uri;
use crate::error::{AppError, AppResult};
use crate::infra::fsio;
use crate::infra::registry::LibraryHandle;

/// Version of the definition above. Part of every preimage and of the side cache.
pub const SCHEME_VERSION: u32 = 1;

/// Assets between two saves of the side cache during a backfill.
pub const DEFAULT_SAVE_EVERY: usize = 250;

// ---- hashing -------------------------------------------------------------------

fn hex_of(hasher: Sha256) -> String {
    format!("{:x}", hasher.finalize())
}

/// A library-relative path that may be hashed: under `library/`, no `.`/`..`
/// parts, no separators of the other platform.
fn managed_rel(rel: &str) -> bool {
    rel.starts_with("library/") && !rel.contains(['\\', ':']) && rel.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Computes content hashes for the files under one library root. Keeps the digest
/// of every shared `_resources` file it has read, so one image used by a thousand
/// models is read once per run.
pub struct ContentHasher {
    root: PathBuf,
    shared: Arc<Mutex<HashMap<String, String>>>,
    /// Digests the caller already knows (a file named by its content), so it is not read.
    known: HashMap<String, String>,
    buffer: Vec<u8>,
}

/// Digests of shared files, for hashers working side by side.
type SharedDigests = Arc<Mutex<HashMap<String, String>>>;

fn line(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(format!("{name}={value}
"));
}

impl ContentHasher {
    pub fn new(root: &Path) -> Self {
        Self::with_shared(root, SharedDigests::default())
    }

    fn with_shared(root: &Path, shared: SharedDigests) -> Self {
        ContentHasher { root: root.to_path_buf(), shared, known: HashMap::new(), buffer: vec![0; 256 * 1024] }
    }

    /// Take `digest` as `D(rel)` instead of reading the file: for a file whose SHA-256 is already known
    /// (it is stored under a path named by it), so a large archive is not read again.
    pub fn assume_digest(&mut self, rel: &str, digest: &str) {
        self.known.insert(rel.to_string(), digest.to_string());
    }

    /// `D(file)`: SHA-256 of a library-relative file, read in chunks.
    pub fn file_digest(&mut self, rel: &str) -> AppResult<String> {
        if !managed_rel(rel) {
            return Err(AppError::msg(format!("not a library file: {rel}")));
        }
        if let Some(digest) = self.known.get(rel) {
            return Ok(digest.clone());
        }
        let shared = rel.contains("/_resources/");
        if shared {
            if let Some(digest) = self.shared.lock().unwrap_or_else(|p| p.into_inner()).get(rel) {
                return Ok(digest.clone());
            }
        }
        let mut path = self.root.clone();
        path.extend(rel.split('/'));
        let mut file = std::fs::File::open(&path).map_err(|error| AppError::file("Read", &path, error))?;
        let mut hasher = Sha256::new();
        loop {
            let count = file.read(&mut self.buffer).map_err(|error| AppError::file("Read", &path, error))?;
            if count == 0 {
                break;
            }
            hasher.update(&self.buffer[..count]);
        }
        let digest = hex_of(hasher);
        if shared {
            self.shared.lock().unwrap_or_else(|p| p.into_inner()).insert(rel.to_string(), digest.clone());
        }
        Ok(digest)
    }

    /// SHA-256 of the normalised glTF at `rel` (see the module documentation).
    pub fn gltf_digest(&mut self, rel: &str) -> AppResult<String> {
        if !managed_rel(rel) {
            return Err(AppError::msg(format!("not a library file: {rel}")));
        }
        let mut path = self.root.clone();
        path.extend(rel.split('/'));
        let mut doc: Value = serde_json::from_slice(&fsio::read_bytes(&path)?)?;
        let parent = rel.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        for field in ["buffers", "images"] {
            let Some(items) = doc.get_mut(field).and_then(Value::as_array_mut) else { continue };
            for item in items {
                let Some(uri) = item.get("uri").and_then(Value::as_str) else { continue };
                // Embedded and remote references stay exactly as written.
                if let Some(file) = gltf_uri::resolve_library_file(parent, uri)? {
                    let digest = self.file_digest(&file)?;
                    item["uri"] = Value::String(format!("sha256:{digest}"));
                }
            }
        }
        strip_identity(&mut doc);
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_vec(&doc)?);
        Ok(hex_of(hasher))
    }

    /// The content hash of one item (its variants have their own). Fails when a
    /// file is missing or unreadable.
    pub fn hash_asset(&mut self, asset: &Asset) -> AppResult<String> {
        let mut hasher = Sha256::new();
        hasher.update(format!("toybox-content-v{SCHEME_VERSION}\n{}\n", asset.content.type_id()));
        match &asset.content {
            AssetContent::Model { fileset, .. } => {
                let digest = self.gltf_digest(&fileset.gltf)?;
                line(&mut hasher, "gltf", &digest);
            }
            AssetContent::Animation { fileset, clip } => {
                let digest = self.gltf_digest(&fileset.gltf)?;
                line(&mut hasher, "gltf", &digest);
                if !clip.standalone {
                    line(&mut hasher, "clip", &clip.index.to_string());
                }
            }
            AssetContent::Material { material } => {
                let mut maps = Vec::with_capacity(material.maps.len());
                for map in &material.maps {
                    let role = serde_json::to_value(map.role)?;
                    let channel = map.channel.map(serde_json::to_value).transpose()?;
                    let digest = self.file_digest(&map.file)?;
                    maps.push(format!(
                        "{}|{}|{}|{digest}",
                        role.as_str().unwrap_or("?"),
                        channel.as_ref().and_then(Value::as_str).unwrap_or("-"),
                        u8::from(map.invert)
                    ));
                }
                maps.sort();
                for map in &maps {
                    line(&mut hasher, "map", map);
                }
                line(&mut hasher, "flipNormalGreen", &u8::from(material.flip_normal_green).to_string());
            }
            AssetContent::Audio { audio } => {
                let digest = self.file_digest(&audio.file)?;
                line(&mut hasher, "file", &digest);
            }
            AssetContent::Plugin { files, .. } => {
                for file in files {
                    let digest = self.file_digest(file)?;
                    line(&mut hasher, "file", &digest);
                }
            }
        }
        Ok(hex_of(hasher))
    }
}

/// Remove what Toybox itself records about where a scene came from: ids and names
/// that would make two copies of the same content differ.
fn strip_identity(doc: &mut Value) {
    fn remove_key(parent: &mut Value, pointer: &str, key: &str) -> bool {
        parent.pointer_mut(pointer).and_then(Value::as_object_mut).is_some_and(|object| object.remove(key).is_some())
    }
    fn is_empty_object(doc: &Value, pointer: &str) -> bool {
        doc.pointer(pointer).and_then(Value::as_object).is_some_and(|object| object.is_empty())
    }
    // The import's staging path (`library/_import_x/<folder>/<file>.gltf`).
    if remove_key(doc, "/extras", "toyboxSourcePath") && is_empty_object(doc, "/extras") {
        doc.as_object_mut().map(|object| object.remove("extras"));
    }
    if remove_key(doc, "/extensions/TOYBOX_library", "sourcePath") && is_empty_object(doc, "/extensions/TOYBOX_library") {
        remove_key(doc, "/extensions", "TOYBOX_library");
        if is_empty_object(doc, "/extensions") {
            doc.as_object_mut().map(|object| object.remove("extensions"));
        }
    }
    // An extracted clip's own id and the id and name of the model it was cut from.
    remove_key(doc, "/extras/toyboxAnimation", "id");
    remove_key(doc, "/extras/toyboxAnimation/clip", "sourceId");
    remove_key(doc, "/extras/toyboxAnimation/clip", "sourceName");
}

// ---- keys ----------------------------------------------------------------------

/// The side-cache key of an item: `<type folder>/<key>` of its content-addressed
/// storage folder (`library/models/<64 hex>/Crate.gltf` -> `models/<64 hex>`).
/// `None` for anything that is not in such a folder (it is then never hashed:
/// nothing guarantees its files do not change).
pub fn storage_key(asset: &Asset) -> Option<String> {
    let mut files = Vec::new();
    asset.content_files(&mut files);
    let first = files.first()?;
    let parts: Vec<&str> = first.split('/').collect();
    if parts.len() < 4 || parts[0] != "library" {
        return None;
    }
    let (folder, key) = (parts[1], parts[2]);
    if folder.is_empty() || folder.starts_with(['_', '.']) || key.len() < 16 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    // Clips cut from one shared file have the folder in common.
    match &asset.content {
        AssetContent::Animation { clip, .. } if !clip.standalone => Some(format!("{folder}/{key}#{}", clip.index)),
        _ => Some(format!("{folder}/{key}")),
    }
}

/// Every item and variant of a catalog, depth first.
fn walk<'a>(assets: &'a [Asset], visit: &mut dyn FnMut(&'a Asset)) {
    for asset in assets {
        visit(asset);
        walk(&asset.variants, visit);
    }
}

fn walk_mut(assets: &mut [Asset], visit: &mut dyn FnMut(&mut Asset)) {
    for asset in assets {
        visit(asset);
        walk_mut(&mut asset.variants, visit);
    }
}

/// The side-cache keys of every item and variant of a catalog.
pub fn keys_of(catalog: &Catalog) -> HashSet<String> {
    let mut keys = HashSet::new();
    walk(&catalog.assets, &mut |asset| {
        keys.extend(storage_key(asset));
    });
    keys
}

/// The storage keys a manifest already records (entries and variants), so the
/// items a commit adds can be told from the ones that were there.
pub fn keys_in_manifest(manifest: &Value) -> HashSet<String> {
    fn add(entry: &Value, keys: &mut HashSet<String>) {
        if let Some(path) = entry.get("storagePath").and_then(Value::as_str) {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 3 && parts[0] == "library" {
                keys.insert(format!("{}/{}", parts[1], parts[2]));
            }
        }
        for variant in entry.get("variants").and_then(Value::as_array).into_iter().flatten() {
            add(variant, keys);
        }
    }
    let mut keys = HashSet::new();
    for entry in manifest.get("assets").and_then(Value::as_array).into_iter().flatten() {
        add(entry, &mut keys);
    }
    keys
}

// ---- the side cache ------------------------------------------------------------

/// The side cache as read: `storage key -> content hash`.
#[derive(Debug, Default, Clone)]
pub struct HashCache {
    hashes: HashMap<String, String>,
}

impl HashCache {
    pub fn get(&self, key: &str) -> Option<&String> {
        self.hashes.get(key)
    }
    pub fn contains(&self, key: &str) -> bool {
        self.hashes.contains_key(key)
    }
    pub fn len(&self) -> usize {
        self.hashes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }
}

/// The side cache of a library: its library database.
pub fn cache_path(handle: &LibraryHandle) -> PathBuf {
    handle.library_db_path()
}

/// Read a side cache. A missing, damaged or other-scheme database is an empty cache.
pub fn load_cache(path: &Path) -> HashCache {
    HashCache { hashes: crate::infra::store::hashes::load(path) }
}

static WRITE: Mutex<()> = Mutex::new(());

/// Set and forget a few keys of the side cache under the same lock as `update_cache`.
fn update_keys(path: &Path, set: &[(String, String)], forget: &[String]) -> AppResult<()> {
    let _guard = WRITE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    crate::infra::store::hashes::update_keys(path, set, forget)
}

// ---- applying ------------------------------------------------------------------

/// Set `content_hash` on every item and variant from the side cache (and clear it
/// where the cache has nothing, so nothing stored elsewhere can be stale).
pub fn apply(catalog: &mut Catalog, cache: &HashCache) {
    walk_mut(&mut catalog.assets, &mut |asset| {
        asset.content_hash = storage_key(asset).and_then(|key| cache.get(&key).cloned());
    });
}

/// [`apply`] with the library's own side cache. A remote library's hashes come
/// from its server inside the catalog and are left alone.
pub fn fill_from_cache(handle: &LibraryHandle, catalog: &mut Catalog) {
    if handle.is_remote() {
        return;
    }
    apply(catalog, &load_cache(&cache_path(handle)));
}

/// Every content hash a catalog carries, in catalog order (for a revision that
/// has to change when fingerprints arrive).
pub fn for_each_hash(catalog: &Catalog, visit: &mut dyn FnMut(&str)) {
    walk(&catalog.assets, &mut |asset| {
        if let Some(hash) = &asset.content_hash {
            visit(hash);
        }
    });
}

/// Items and variants that have a storage key but no hash in `catalog`.
pub fn count_unhashed(catalog: &Catalog) -> usize {
    let mut count = 0;
    walk(&catalog.assets, &mut |asset| {
        if asset.content_hash.is_none() && storage_key(asset).is_some() {
            count += 1;
        }
    });
    count
}

/// Set `content_hash` on these items and their variants from the side cache,
/// looking up only their own storage keys (a remote library's come with its
/// catalog and are left alone).
pub fn fill_assets(handle: &LibraryHandle, assets: &mut [Asset]) {
    if handle.is_remote() || assets.is_empty() {
        return;
    }
    let mut keys = Vec::new();
    walk(assets, &mut |asset| keys.extend(storage_key(asset)));
    let found = crate::infra::store::hashes::get_keys(&cache_path(handle), &keys);
    walk_mut(assets, &mut |asset| {
        asset.content_hash = storage_key(asset).and_then(|key| found.get(&key).cloned());
    });
}

/// The hashes these items and variants are missing, computed from their files
/// (nothing is written). Items whose storage key is in the side cache are not
/// read again; shared images are read once. Failures are skipped: such an item
/// stays unhashed until a backfill finds it readable.
pub fn missing_hashes(root: &Path, db: &Path, assets: &[Asset]) -> Vec<(String, String)> {
    let mut keys = Vec::new();
    walk(assets, &mut |asset| keys.extend(storage_key(asset)));
    let known = crate::infra::store::hashes::get_keys(db, &keys);
    let mut hasher = ContentHasher::new(root);
    let mut fresh: HashMap<String, String> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    walk(assets, &mut |asset| {
        let Some(key) = storage_key(asset) else { return };
        if known.contains_key(&key) || fresh.contains_key(&key) {
            return;
        }
        if let Ok(hash) = hasher.hash_asset(asset) {
            order.push(key.clone());
            fresh.insert(key, hash);
        }
    });
    order.into_iter().map(|key| { let hash = fresh.remove(&key).unwrap_or_default(); (key, hash) }).collect()
}

/// After a write: hash the given items (and variants) that the side cache has no
/// hash for, save the hashes, and set `content_hash` on all of them. Only these
/// items' files and keys are touched. Returns how many hashes were added.
pub fn hash_new(handle: &LibraryHandle, assets: &mut [Asset]) -> usize {
    if handle.is_remote() {
        return 0;
    }
    let fresh = missing_hashes(&handle.root, &cache_path(handle), assets);
    let added = fresh.len();
    if added > 0 {
        let _ = update_keys(&cache_path(handle), &fresh, &[]);
    }
    fill_assets(handle, assets);
    added
}

/// Forget and recompute the hash of items whose stored files were edited in place.
pub fn rehash(handle: &LibraryHandle, assets: &[&Asset]) {
    if handle.is_remote() {
        return;
    }
    let path = cache_path(handle);
    let mut hasher = ContentHasher::new(&handle.root);
    let mut fresh = HashMap::new();
    let mut stale = Vec::new();
    for asset in assets {
        let Some(key) = storage_key(asset) else { continue };
        match hasher.hash_asset(asset) {
            Ok(hash) => {
                fresh.insert(key, hash);
            }
            Err(_) => stale.push(key),
        }
    }
    let set: Vec<(String, String)> = fresh.into_iter().collect();
    let _ = update_keys(&path, &set, &stale);
}

/// A catalog of just the items that lack a content hash (see
/// `Store::assets_to_hash`), with the hashes that are there filled in, so
/// [`count_unhashed`] and [`backfill`] see only the work to do. The rest of the
/// library is not read.
pub fn unhashed_catalog(handle: &LibraryHandle) -> AppResult<Catalog> {
    let store = crate::infra::store::Store::for_handle(handle);
    let mut assets = store.assets_to_hash()?;
    fill_assets(handle, &mut assets);
    let mut pending = Vec::new();
    for asset in assets {
        let mut missing = false;
        walk(std::slice::from_ref(&asset), &mut |item| missing |= item.content_hash.is_none() && storage_key(item).is_some());
        if missing {
            pending.push(asset);
        }
    }
    Ok(Catalog { rigs: Vec::new(), schema_version: crate::config::SCHEMA_VERSION, library_root: handle.root_str(), assets: pending })
}

/// One page of [`unhashed_catalog`]: up to `limit` assets after the id `after`
/// that lack a hash, and where to continue. Nothing else of the library is read.
pub fn unhashed_page(handle: &LibraryHandle, after: &str, limit: usize) -> AppResult<(Catalog, Option<String>)> {
    let store = crate::infra::store::Store::for_handle(handle);
    let (mut assets, next) = store.pending_hash_assets(after, limit)?;
    fill_assets(handle, &mut assets);
    let mut pending = Vec::new();
    for asset in assets {
        let mut missing = false;
        walk(std::slice::from_ref(&asset), &mut |item| missing |= item.content_hash.is_none() && storage_key(item).is_some());
        if missing {
            pending.push(asset);
        }
    }
    Ok((Catalog { rigs: Vec::new(), schema_version: crate::config::SCHEMA_VERSION, library_root: handle.root_str(), assets: pending }, next))
}

/// Drop the side-cache entries whose item is gone from the catalog and whose
/// folder is gone from disk. Returns how many were dropped. Reads the keys only.
pub fn prune_hashes(handle: &LibraryHandle) -> AppResult<usize> {
    let store = crate::infra::store::Store::for_handle(handle);
    let gone: Vec<String> = store
        .orphaned_hash_keys()?
        .into_iter()
        .filter(|key| !handle.root.join("library").join(key.split('#').next().unwrap_or(key)).exists())
        .collect();
    if !gone.is_empty() {
        update_keys(&cache_path(handle), &[], &gone)?;
    }
    Ok(gone.len())
}

// ---- backfill ------------------------------------------------------------------

/// What a backfill reports while it runs.
#[derive(Debug)]
pub enum Event<'a> {
    /// How many items need a hash in this run.
    Started { total: usize },
    /// `done` of `total` items looked at (hashed or skipped).
    Progress { done: usize, total: usize },
    /// The side cache was saved with these `(asset id, hash)` pairs (ids of
    /// this library, variants included).
    Saved { hashes: &'a [(String, String)] },
}

pub struct BackfillOptions<'a> {
    /// Items between two saves of the side cache (and so between two `Saved` events).
    pub save_every: usize,
    /// Stop after this many items looked at; the rest is left for a later call.
    pub max_items: Option<usize>,
    /// Storage keys not to try (they failed earlier in the same session).
    pub skip: &'a HashSet<String>,
    /// Drop side-cache entries whose item is gone from the catalog and whose folder is gone from disk.
    pub prune: bool,
    /// Items hashed side by side (at least 1). Reading files is usually the slow part.
    pub threads: usize,
}

impl Default for BackfillOptions<'_> {
    fn default() -> Self {
        static NONE: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
        BackfillOptions { save_every: DEFAULT_SAVE_EVERY, max_items: None, skip: NONE.get_or_init(HashSet::new), prune: false, threads: 1 }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BackfillReport {
    /// Items that needed a hash when the call started.
    pub total: usize,
    /// Looked at in this call.
    pub attempted: usize,
    /// Hashed and saved.
    pub hashed: usize,
    /// Storage keys that could not be hashed (missing or unreadable files).
    pub failed: Vec<String>,
    pub cancelled: bool,
}

/// Compute the hashes the side cache is missing for `catalog`. Resumable (what is
/// saved is never redone), cancellable, and safe to run while the library is read
/// or written: it only reads files, and an item whose files are gone is skipped,
/// not failed. Shared images are read once.
pub fn backfill(
    root: &Path,
    cache_file: &Path,
    catalog: &Catalog,
    cancel: &AtomicBool,
    options: &BackfillOptions,
    events: &mut dyn FnMut(Event),
) -> AppResult<BackfillReport> {
    // Only the keys of this catalog are asked of the side cache, unless pruning has to see it all.
    let cache = if options.prune {
        load_cache(cache_file)
    } else {
        let mut keys = Vec::new();
        walk(&catalog.assets, &mut |asset| keys.extend(storage_key(asset)));
        HashCache { hashes: crate::infra::store::hashes::get_keys(cache_file, &keys) }
    };
    if options.prune {
        let live = keys_of(catalog);
        let gone: Vec<String> = cache
            .hashes
            .keys()
            .filter(|key| !live.contains(key.as_str()) && !root.join("library").join(key.split('#').next().unwrap_or(key)).exists())
            .cloned()
            .collect();
        if !gone.is_empty() {
            update_keys(cache_file, &[], &gone)?;
        }
    }
    // Pending items, once per storage key (clips or a variant sharing a folder share a hash).
    let mut order: Vec<(String, &Asset)> = Vec::new();
    let mut ids: HashMap<String, Vec<String>> = HashMap::new();
    walk(&catalog.assets, &mut |asset| {
        let Some(key) = storage_key(asset) else { return };
        if cache.contains(&key) || options.skip.contains(&key) {
            return;
        }
        if !ids.contains_key(&key) {
            order.push((key.clone(), asset));
        }
        ids.entry(key).or_default().push(asset.id.clone());
    });
    let total = order.len();
    let mut report = BackfillReport { total, ..Default::default() };
    events(Event::Started { total });
    let mut batch: HashMap<String, String> = HashMap::new();
    let mut announce: Vec<(String, String)> = Vec::new();
    let save_every = options.save_every.max(1);
    let count = order.len().min(options.max_items.unwrap_or(usize::MAX));

    let flush = |batch: &mut HashMap<String, String>, announce: &mut Vec<(String, String)>, events: &mut dyn FnMut(Event)| -> AppResult<()> {
        if batch.is_empty() {
            return Ok(());
        }
        let pending: Vec<(String, String)> = std::mem::take(batch).into_iter().collect();
        update_keys(cache_file, &pending, &[])?;
        events(Event::Saved { hashes: announce });
        announce.clear();
        Ok(())
    };

    let shared = SharedDigests::default();
    let workers = options.threads.max(1).min(count.max(1));
    // The books: one place records a result, saves the side cache and reports.
    let mut record = |index: usize, result: AppResult<String>| -> AppResult<()> {
        let key = order[index].0.clone();
        match result {
            Ok(hash) => {
                for id in ids.remove(&key).unwrap_or_default() {
                    announce.push((id, hash.clone()));
                }
                batch.insert(key, hash);
                report.hashed += 1;
            }
            Err(_) => report.failed.push(key),
        }
        report.attempted += 1;
        events(Event::Progress { done: report.attempted, total });
        if batch.len() >= save_every {
            flush(&mut batch, &mut announce, events)?;
        }
        Ok(())
    };
    if workers == 1 {
        let mut hasher = ContentHasher::with_shared(root, shared);
        for index in 0..count {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            record(index, hasher.hash_asset(order[index].1))?;
        }
    } else {
        // Workers pull items in order and send their results back; this thread alone
        // keeps the books. A cancelled run finishes the items already under way.
        let next = AtomicUsize::new(0);
        let (sender, receiver) = mpsc::channel::<(usize, AppResult<String>)>();
        std::thread::scope(|scope| -> AppResult<()> {
            for _ in 0..workers {
                let sender = sender.clone();
                let (order, next, shared) = (&order, &next, shared.clone());
                scope.spawn(move || {
                    let mut hasher = ContentHasher::with_shared(root, shared);
                    loop {
                        let index = next.fetch_add(1, Ordering::SeqCst);
                        if index >= count || cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        if sender.send((index, hasher.hash_asset(order[index].1))).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(sender);
            for (index, result) in receiver {
                if let Err(error) = record(index, result) {
                    next.store(usize::MAX / 2, Ordering::SeqCst);
                    return Err(error);
                }
            }
            Ok(())
        })?;
    }
    // Whatever was recorded but not yet saved.
    report.cancelled = cancel.load(Ordering::Relaxed) && report.attempted < count;
    flush(&mut batch, &mut announce, events)?;
    Ok(report)
}
