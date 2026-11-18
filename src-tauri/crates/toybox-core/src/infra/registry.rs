//! The library registry: which libraries the user has added, which one imports
//! go to, and where each one keeps its per-library app data. Everything here is
//! plain data and plain paths (no Tauri handle), so the migrations and the
//! registry operations are unit-testable on temp dirs. `infra::library` wraps
//! these with the live state and the asset-protocol scope.
//!
//! App-data layout, per library id:
//!   `<app-data>/libraries/<id>/user.db`        the user's favourites, tags, collections
//!   `<app-data>/libraries/<id>/library.db`     remote libraries only: the replica of
//!                                              the server's catalog (a local library's
//!                                              lives in its own folder)
//!   `<app-data>/libraries/<id>/catalog.json`   the cache of earlier versions; the
//!                                              library migration (step 4) moves its
//!                                              content into the databases
//!   `<app-data>/libraries/<id>/thumbs/`        thumbnails
//!   `<app-data>/libraries/<id>/library.json`   `{ root, name }` sidecar, so a
//!                                              removed library's folder can be
//!                                              recognised when it is added again
//!
//! A library's id is a short random string made when it is added. It is never
//! derived from the path, so a library can later move or be served from
//! elsewhere without its identity changing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::infra::settings::AppSettings;
use crate::infra::{appdata, fsio};

/// What kind of library an entry is: a folder on this machine, or a library
/// served by a `toybox-server` (address in `url`, access token in the OS
/// credential store, never in settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LibraryKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub id: String,
    pub name: String,
    pub kind: LibraryKind,
    /// Absolute folder of a local library. Empty for a remote one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root: String,
    /// Normalised server address of a remote library (`https://host[:port]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl LibraryEntry {
    pub fn local(id: &str, name: &str, root: &str) -> Self {
        LibraryEntry { id: id.into(), name: name.into(), kind: LibraryKind::Local, root: root.into(), url: None }
    }
    pub fn remote(id: &str, name: &str, url: &str) -> Self {
        LibraryEntry { id: id.into(), name: name.into(), kind: LibraryKind::Remote, root: String::new(), url: Some(url.into()) }
    }
}

/// Why tools that rewrite a library's files in place (origin alignment) are refused on a remote library.
pub const REMOTE_NO_FILE_EDITS: &str = "Editing files in place is not supported for remote libraries yet. Copy the item to a local library to edit it there.";

/// What a remote library lets this client do with its files on demand. Core
/// only knows this seam; `toybox-remote` implements it with HTTP.
pub trait FileProvider: Send + Sync + std::fmt::Debug {
    /// Make these library-relative files exist under the handle root, downloading
    /// the missing ones. Paths are validated by the implementation.
    fn ensure_files(&self, rels: &[String]) -> AppResult<()>;
    /// Whether the server holds a shared thumbnail for this asset or variant id.
    fn has_shared_thumb(&self, asset_id: &str) -> bool;
    /// Fetch the shared thumbnail into the thumbs folder if it is not there yet.
    /// `Ok(false)` when the server has none.
    fn ensure_thumb(&self, asset_id: &str) -> AppResult<bool>;
    /// A thumbnail was rendered here: offer it to the server when the role allows.
    fn share_thumb(&self, asset_id: &str, png: &[u8]);
}

/// The remote half of a handle.
#[derive(Debug, Clone)]
pub struct RemoteLink {
    pub url: String,
    pub files: std::sync::Arc<dyn FileProvider>,
}

/// Everything a command needs to work with one library. The only place paths
/// for a library are derived; commands never join `app-data` themselves.
///
/// For a remote library `root` is the local file cache (same library-relative
/// layout as the server), so every path-based reader works on downloaded files;
/// `remote` is how missing ones are fetched.
#[derive(Debug, Clone)]
pub struct LibraryHandle {
    pub id: String,
    pub kind: LibraryKind,
    pub root: PathBuf,
    pub cache_path: PathBuf,
    pub thumbs_dir: PathBuf,
    pub remote: Option<RemoteLink>,
}

impl LibraryHandle {
    pub fn local(app_data: &Path, entry: &LibraryEntry) -> Self {
        let dir = library_dir(app_data, &entry.id);
        LibraryHandle {
            id: entry.id.clone(),
            kind: entry.kind,
            root: PathBuf::from(&entry.root),
            cache_path: dir.join("catalog.json"),
            thumbs_dir: dir.join("thumbs"),
            remote: None,
        }
    }
    /// A remote library: app data under `<app-data>/libraries/<id>` as for any
    /// library, files cached in `files_root`.
    pub fn remote(app_data: &Path, entry: &LibraryEntry, files_root: &Path, link: RemoteLink) -> Self {
        let dir = library_dir(app_data, &entry.id);
        LibraryHandle {
            id: entry.id.clone(),
            kind: LibraryKind::Remote,
            root: files_root.to_path_buf(),
            cache_path: dir.join("catalog.json"),
            thumbs_dir: dir.join("thumbs"),
            remote: Some(link),
        }
    }
    pub fn is_remote(&self) -> bool {
        self.kind == LibraryKind::Remote
    }
    /// The library database: `<library>/_library_config/library.db` for a local
    /// library, a replica beside the cache for a remote one (its manifest lives on the server).
    pub fn library_db_path(&self) -> PathBuf {
        if self.is_remote() { self.data_dir().join("library.db") } else { self.root.join(crate::config::LIBRARY_DB_REL) }
    }
    /// This computer's personal data for the library (favourites, personal tags, ...).
    pub fn user_db_path(&self) -> PathBuf {
        self.data_dir().join("user.db")
    }
    /// Make sure these library-relative files are on disk (a no-op for a local
    /// library). Call before reading files by path.
    pub fn ensure_files(&self, rels: &[String]) -> AppResult<()> {
        match &self.remote {
            Some(link) if !rels.is_empty() => link.files.ensure_files(rels),
            _ => Ok(()),
        }
    }
    /// Make sure the content files of these catalog items (and their variants) are
    /// on disk. Source files kept in `origin.files` are not content; ask for them by path.
    pub fn ensure_assets(&self, assets: &[crate::domain::catalog_model::Asset]) -> AppResult<()> {
        if self.remote.is_none() { return Ok(()); }
        let mut rels = Vec::new();
        for asset in assets { asset.collect_content_files(&mut rels); }
        rels.sort();
        rels.dedup();
        self.ensure_files(&rels)
    }
    /// Make sure a glTF at `rel` and the buffer and images it names are on disk.
    pub fn ensure_gltf(&self, rel: &str) -> AppResult<()> {
        if self.remote.is_none() { return Ok(()); }
        self.ensure_files(&[rel.to_string()])?;
        let path = crate::domain::paths::abs_under_root(&self.root_str(), rel);
        let doc = crate::domain::gltf_parse::parse(&crate::infra::fsio::read_text(&path)?)?;
        let parent = rel.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("").to_string();
        let mut deps = Vec::new();
        for reference in crate::domain::gltf_parse::buffer_uris(&doc).into_iter().map(|r| r.uri).chain(crate::domain::gltf_parse::image_uris(&doc).into_iter().map(|r| r.uri)) {
            // Embedded and remote references are not files to fetch.
            if let Ok(Some(dep)) = crate::domain::gltf_uri::resolve_library_file(&parent, &reference) { deps.push(dep); }
        }
        self.ensure_files(&deps)
    }
    pub fn root_str(&self) -> String {
        self.root.to_string_lossy().to_string()
    }
    /// The PNG of one asset's thumbnail. Ids contain `uid://` and other unsafe
    /// characters, so they are flattened into a filename.
    pub fn thumb_path(&self, asset_id: &str) -> PathBuf {
        self.thumbs_dir.join(format!("{}.png", appdata::sanitize_id(asset_id)))
    }
    /// Whether a thumbnail is on disk or (remote) available from the server.
    pub fn has_thumb(&self, asset_id: &str) -> bool {
        fsio::exists(&self.thumb_path(asset_id)) || self.remote.as_ref().is_some_and(|link| link.files.has_shared_thumb(asset_id))
    }
    /// The per-library app-data folder (cache, thumbs, sidecar, markers).
    pub fn data_dir(&self) -> PathBuf {
        self.cache_path.parent().map(Path::to_path_buf).unwrap_or_default()
    }
}

pub fn libraries_dir(app_data: &Path) -> PathBuf {
    app_data.join("libraries")
}
pub fn library_dir(app_data: &Path, id: &str) -> PathBuf {
    libraries_dir(app_data).join(id)
}

/// A new short random id (10 lowercase hex digits). No random-number crate is
/// available, so entropy comes from the clock, the process, a counter and the
/// randomly seeded std hasher.
pub fn new_id() -> String {
    use std::hash::{BuildHasher, Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    COUNTER.fetch_add(1, Ordering::Relaxed).hash(&mut hasher);
    format!("{:010x}", hasher.finish() & 0xff_ffff_ffff)
}

/// Whether two folder strings name the same folder: canonical paths when both
/// exist, otherwise a separator- and case-insensitive (Windows) comparison.
pub fn same_root(a: &str, b: &str) -> bool {
    if let (Ok(x), Ok(y)) = (Path::new(a).canonicalize(), Path::new(b).canonicalize()) {
        return x == y;
    }
    let norm = |s: &str| {
        let s = s.replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    norm(a) == norm(b)
}

fn folder_name(root: &str) -> String {
    Path::new(root).file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Library".to_string())
}

/// One-time settings migration: an old `libraryRoot` becomes the first library
/// and the default, saved exports scoped by that root are re-scoped by library
/// id, and `libraryRoot` is cleared (it is no longer written). Idempotent: with
/// a registry present nothing changes. Returns whether settings changed.
pub fn migrate_settings(settings: &mut AppSettings, make_id: impl FnOnce() -> String) -> bool {
    let mut changed = false;
    if settings.libraries.is_empty() {
        if let Some(root) = settings.library_root.clone().filter(|r| !r.trim().is_empty()) {
            let id = make_id();
            settings.libraries.push(LibraryEntry::local(&id, &folder_name(&root), &root));
            settings.default_library = Some(id);
            changed = true;
        }
    }
    if settings.library_root.take().is_some() { changed = true; }
    // Saved exports remember the library by id now; theirs recorded its root.
    for export in &mut settings.saved_exports {
        let Some(object) = export.as_object_mut() else { continue };
        let Some(root) = object.get("libraryRoot").and_then(|v| v.as_str()).map(String::from) else { continue };
        if let Some(entry) = settings.libraries.iter().find(|entry| entry.kind == LibraryKind::Local && same_root(&entry.root, &root)) {
            object.insert("libraryId".into(), serde_json::Value::String(entry.id.clone()));
            object.remove("libraryRoot");
            changed = true;
        }
    }
    if settings.default_library.as_ref().map_or(true, |id| !settings.libraries.iter().any(|l| &l.id == id)) {
        let first = settings.libraries.first().map(|l| l.id.clone());
        if first != settings.default_library { settings.default_library = first; changed = true; }
    }
    changed
}

/// Move the single-library app data (`catalog.json`, `thumbs/`) into the folder
/// of the library that owns it. The owner is the registered library whose root
/// the old cache records; with no readable cache it is the only registered
/// library. Safe to interrupt and to repeat: thumbnails move first and the cache
/// last, because the cache is what names the owner, and each step is a rename
/// that is skipped when its source is gone. Nothing is ever overwritten; an
/// older copy that would collide is renamed `*.legacy` instead.
pub fn migrate_app_data(app_data: &Path, settings: &AppSettings) -> AppResult<()> {
    let old_cache = app_data.join("catalog.json");
    let old_thumbs = app_data.join("thumbs");
    if !old_cache.exists() && !old_thumbs.exists() { return Ok(()); }
    let owner_root = std::fs::read(&old_cache).ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|doc| doc["libraryRoot"].as_str().map(String::from));
    let owner = match owner_root {
        Some(root) => settings.libraries.iter().find(|l| l.kind == LibraryKind::Local && same_root(&l.root, &root)),
        None if settings.libraries.len() == 1 => settings.libraries.first().filter(|l| l.kind == LibraryKind::Local),
        None => None,
    };
    let Some(owner) = owner else { return Ok(()) };
    let handle = LibraryHandle::local(app_data, owner);
    std::fs::create_dir_all(handle.data_dir())?;
    write_sidecar(app_data, owner)?;
    move_aside(&old_thumbs, &handle.thumbs_dir)?;
    move_aside(&old_cache, &handle.cache_path)?;
    // The preview-refresh marker used to live in settings.json.
    if settings.animation_preview_version > 0 { set_marker(&handle.data_dir(), PREVIEW_MARKER, settings.animation_preview_version)?; }
    Ok(())
}

/// Rename `from` to `to` unless `to` exists, in which case `from` is kept as
/// `<from>.legacy` so nothing is lost and the migration is not repeated.
fn move_aside(from: &Path, to: &Path) -> AppResult<()> {
    if !from.exists() { return Ok(()); }
    if to.exists() {
        let mut legacy = from.as_os_str().to_owned();
        legacy.push(".legacy");
        let legacy = PathBuf::from(legacy);
        if legacy.exists() { return Ok(()); }
        return std::fs::rename(from, &legacy).map_err(|e| AppError::file("Keep old app data", from, e));
    }
    std::fs::rename(from, to).map_err(|e| AppError::file("Move app data", from, e))
}

pub const PREVIEW_MARKER: &str = "animation-preview-version";

/// Per-library numeric marker file (replaces a global setting that could only
/// ever describe one library).
pub fn marker(dir: &Path, name: &str) -> u32 {
    std::fs::read_to_string(dir.join(name)).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0)
}
pub fn set_marker(dir: &Path, name: &str, value: u32) -> AppResult<()> {
    fsio::write_bytes_atomic(&dir.join(name), value.to_string().as_bytes())
}

pub fn write_sidecar(app_data: &Path, entry: &LibraryEntry) -> AppResult<()> {
    let doc = serde_json::json!({ "root": entry.root, "name": entry.name, "url": entry.url });
    fsio::write_bytes_atomic(&library_dir(app_data, &entry.id).join("library.json"), &serde_json::to_vec_pretty(&doc)?)
}

/// The id of an unregistered library whose app data is still kept and whose
/// recorded folder is `root`: re-adding a removed folder gets its favourites,
/// tags and thumbnails back.
pub fn retained_id_for(app_data: &Path, settings: &AppSettings, root: &str) -> Option<String> {
    for entry in std::fs::read_dir(libraries_dir(app_data)).ok()?.flatten() {
        let id = entry.file_name().to_string_lossy().to_string();
        if settings.libraries.iter().any(|l| l.id == id) { continue; }
        let Ok(bytes) = std::fs::read(entry.path().join("library.json")) else { continue };
        let Ok(doc) = serde_json::from_slice::<serde_json::Value>(&bytes) else { continue };
        if doc["root"].as_str().is_some_and(|kept| same_root(kept, root)) { return Some(id); }
    }
    None
}

impl AppSettings {
    pub fn library(&self, id: &str) -> Option<&LibraryEntry> {
        self.libraries.iter().find(|l| l.id == id)
    }

    /// Register a local library. The first one becomes the default. `id` is used
    /// as given (a kept id from `retained_id_for`, or a fresh one).
    pub fn add_library(&mut self, root: &str, name: Option<&str>, id: String) -> AppResult<LibraryEntry> {
        if let Some(existing) = self.libraries.iter().find(|l| l.kind == LibraryKind::Local && same_root(&l.root, root)) {
            return Err(AppError::msg(format!("This folder is already in your libraries as \"{}\"", existing.name)));
        }
        if self.libraries.iter().any(|l| l.id == id) { return Err(AppError::msg("Library id is already in use")); }
        let name = name.map(str::trim).filter(|n| !n.is_empty()).map(String::from).unwrap_or_else(|| folder_name(root));
        let entry = LibraryEntry::local(&id, &name, root);
        self.libraries.push(entry.clone());
        if self.default_library.is_none() { self.default_library = Some(id); }
        Ok(entry)
    }

    /// Register a remote library. The same server may be added more than once
    /// (for example with a read-only and a write token); each gets its own id.
    /// The token is not part of the entry: it lives in the OS credential store.
    pub fn add_remote_library(&mut self, url: &str, name: &str, id: String) -> AppResult<LibraryEntry> {
        if self.libraries.iter().any(|l| l.id == id) { return Err(AppError::msg("Library id is already in use")); }
        let name = name.trim();
        if name.is_empty() { return Err(AppError::msg("A library needs a name")); }
        let entry = LibraryEntry::remote(&id, name, url);
        self.libraries.push(entry.clone());
        if self.default_library.is_none() { self.default_library = Some(id); }
        Ok(entry)
    }

    pub fn rename_library(&mut self, id: &str, name: &str) -> AppResult<LibraryEntry> {
        let name = name.trim();
        if name.is_empty() { return Err(AppError::msg("A library needs a name")); }
        let entry = self.libraries.iter_mut().find(|l| l.id == id).ok_or_else(|| AppError::msg("Unknown library"))?;
        entry.name = name.to_string();
        Ok(entry.clone())
    }

    /// Unregister a library. Its folder is never touched. If it was the default,
    /// the first remaining library takes over.
    pub fn remove_library(&mut self, id: &str) -> AppResult<LibraryEntry> {
        let at = self.libraries.iter().position(|l| l.id == id).ok_or_else(|| AppError::msg("Unknown library"))?;
        let removed = self.libraries.remove(at);
        if self.default_library.as_deref() == Some(id) { self.default_library = self.libraries.first().map(|l| l.id.clone()); }
        Ok(removed)
    }

    pub fn set_default_library(&mut self, id: &str) -> AppResult<()> {
        if self.library(id).is_none() { return Err(AppError::msg("Unknown library")); }
        self.default_library = Some(id.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("toybox_registry_{tag}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }
    fn s(path: &Path) -> String { path.to_string_lossy().to_string() }

    #[test]
    fn old_library_root_becomes_the_first_and_default_library_and_is_no_longer_written() {
        let mut settings: AppSettings = serde_json::from_value(json!({
            "libraryRoot": "D:/assets/MyLib", "fbx2gltfPath": "D:/convert.exe",
            "savedExports": [{ "id": "a", "libraryRoot": "D:\\assets\\MyLib\\", "lastAssets": { "uid://x": "h" } }, { "id": "b", "libraryRoot": "E:/other" }]
        })).unwrap();
        assert!(migrate_settings(&mut settings, || "abc".into()));
        assert_eq!(settings.libraries, vec![LibraryEntry::local("abc", "MyLib", "D:/assets/MyLib")]);
        assert_eq!(settings.default_library.as_deref(), Some("abc"));
        assert!(settings.library_root.is_none());
        // Saved export on that root is re-scoped by id and keeps its raw-id fingerprints; the unrelated one is left alone.
        assert_eq!(settings.saved_exports[0]["libraryId"], "abc");
        assert!(settings.saved_exports[0].get("libraryRoot").is_none());
        assert_eq!(settings.saved_exports[0]["lastAssets"]["uid://x"], "h");
        assert_eq!(settings.saved_exports[1]["libraryRoot"], "E:/other");
        let text = serde_json::to_string(&settings).unwrap();
        assert!(!text.contains("\"libraryRoot\":\"D:"), "{text}");
        assert!(text.contains("\"defaultLibrary\":\"abc\"") && text.contains("\"kind\":\"local\""));
        assert_eq!(settings.fbx2gltf_path.as_deref(), Some("D:/convert.exe"));
        // Idempotent: a second run (or a restart) changes nothing and mints no new id.
        let again = serde_json::to_value(&settings).unwrap();
        let mut reloaded: AppSettings = serde_json::from_value(again.clone()).unwrap();
        assert!(!migrate_settings(&mut reloaded, || panic!("no new id")));
        assert_eq!(serde_json::to_value(&reloaded).unwrap(), again);
    }

    #[test]
    fn settings_without_any_library_stay_empty() {
        let mut settings = AppSettings::default();
        assert!(!migrate_settings(&mut settings, || panic!("no id")));
        assert!(settings.libraries.is_empty() && settings.default_library.is_none());
    }

    #[test]
    fn registry_operations_keep_ids_and_a_valid_default() {
        let mut settings = AppSettings::default();
        let a = settings.add_library("D:/one", None, "aaa".into()).unwrap();
        assert_eq!(a.name, "one");
        assert_eq!(settings.default_library.as_deref(), Some("aaa"), "the first library is the default");
        let b = settings.add_library("E:/two", Some(" Second "), "bbb".into()).unwrap();
        assert_eq!(b.name, "Second");
        assert_eq!(settings.default_library.as_deref(), Some("aaa"));
        // Windows folders are case-insensitive; elsewhere only the separators are folded.
        let again = if cfg!(windows) { "e:\\two\\" } else { "E:\\two\\" };
        assert!(settings.add_library(again, None, "ccc".into()).is_err(), "same folder twice");
        assert!(settings.add_library("F:/three", None, "bbb".into()).is_err(), "duplicate id");
        settings.rename_library("bbb", "Archive").unwrap();
        assert!(settings.rename_library("bbb", "  ").is_err());
        assert!(settings.rename_library("nope", "x").is_err());
        assert_eq!(settings.library("bbb").unwrap().id, "bbb", "renaming never changes the id");
        settings.set_default_library("bbb").unwrap();
        assert!(settings.set_default_library("nope").is_err());
        settings.remove_library("bbb").unwrap();
        assert_eq!(settings.default_library.as_deref(), Some("aaa"), "default falls to the remaining library");
        assert!(settings.remove_library("bbb").is_err());
        settings.remove_library("aaa").unwrap();
        assert!(settings.libraries.is_empty() && settings.default_library.is_none());
    }

    #[test]
    fn a_remote_entry_stores_an_address_and_never_a_token() {
        let mut settings = AppSettings::default();
        settings.add_library("D:/one", None, "aaa".into()).unwrap();
        let remote = settings.add_remote_library("https://assets.example.com", " Friends' assets ", "rrr".into()).unwrap();
        assert_eq!((remote.kind, remote.name.as_str(), remote.url.as_deref(), remote.root.as_str()), (LibraryKind::Remote, "Friends' assets", Some("https://assets.example.com"), ""));
        assert!(settings.add_remote_library("https://x.example.com", "  ", "r2".into()).is_err());
        assert!(settings.add_remote_library("https://x.example.com", "Same id", "rrr".into()).is_err());
        // The same server twice (a read and a write token) is two libraries.
        assert!(settings.add_remote_library("https://assets.example.com", "Editor", "r3".into()).is_ok());
        let text = serde_json::to_string(&settings).unwrap();
        assert!(text.contains("\"kind\":\"remote\"") && text.contains("\"url\":\"https://assets.example.com\""));
        assert!(!text.contains("token") && !text.contains("secret"));
        let back: AppSettings = serde_json::from_str(&text).unwrap();
        assert_eq!(back.libraries, settings.libraries);
        assert_eq!(back.library("rrr").unwrap().root, "", "a remote entry has no folder");
        // The local-only rules ignore it: the same folder is not "already added" by a remote entry.
        let mut again = back;
        assert!(again.add_library("D:/two", None, "bbb".into()).is_ok());
        // A remote entry has no folder to be recognised by when removed and re-added.
        assert_eq!(retained_id_for(Path::new("/nowhere"), &again, "D:/two"), None);
        // Removing it only edits the list.
        again.remove_library("rrr").unwrap();
        assert!(again.library("rrr").is_none());
    }

    #[test]
    fn remote_handles_keep_per_library_app_data_and_a_separate_file_cache() {
        struct Nothing;
        impl std::fmt::Debug for Nothing {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("Nothing") }
        }
        impl FileProvider for Nothing {
            fn ensure_files(&self, _: &[String]) -> AppResult<()> { Ok(()) }
            fn has_shared_thumb(&self, id: &str) -> bool { id == "uid://shared" }
            fn ensure_thumb(&self, _: &str) -> AppResult<bool> { Ok(false) }
            fn share_thumb(&self, _: &str, _: &[u8]) {}
        }
        let data = Path::new("/appdata");
        let entry = LibraryEntry::remote("rrr", "Cloud", "https://assets.example.com");
        let handle = LibraryHandle::remote(data, &entry, Path::new("/cache/rrr/files"), RemoteLink { url: "https://assets.example.com".into(), files: std::sync::Arc::new(Nothing) });
        assert!(handle.is_remote());
        assert_eq!(handle.cache_path, data.join("libraries/rrr/catalog.json"));
        assert_eq!(handle.thumbs_dir, data.join("libraries/rrr/thumbs"));
        assert_eq!(handle.root, Path::new("/cache/rrr/files"));
        assert!(handle.has_thumb("uid://shared") && !handle.has_thumb("uid://other"), "a shared picture counts without being on disk");
        let local = LibraryHandle::local(data, &LibraryEntry::local("aaa", "A", "D:/a"));
        assert!(!local.is_remote() && !local.has_thumb("uid://shared"));
        assert!(local.ensure_files(&["library/a/b".into()]).is_ok(), "nothing to fetch for a local library");
    }

    #[test]
    fn ids_are_short_random_and_not_path_derived() {
        let ids: std::collections::HashSet<_> = (0..200).map(|_| new_id()).collect();
        assert_eq!(ids.len(), 200);
        assert!(ids.iter().all(|id| id.len() == 10 && id.bytes().all(|b| b.is_ascii_hexdigit())));
    }

    #[test]
    fn handles_resolve_each_library_to_its_own_cache_and_thumbs() {
        let data = Path::new("/appdata");
        let one = LibraryHandle::local(data, &LibraryEntry::local("aaa", "A", "D:/a"));
        let two = LibraryHandle::local(data, &LibraryEntry::local("bbb", "B", "E:/b"));
        assert_eq!(one.cache_path, data.join("libraries/aaa/catalog.json"));
        assert_eq!(two.thumbs_dir, data.join("libraries/bbb/thumbs"));
        assert_ne!(one.thumb_path("uid://same"), two.thumb_path("uid://same"));
        assert!(one.thumb_path("uid://same").ends_with("uid___same.png"));
    }

    fn legacy_fixture(tag: &str) -> (Tmp, AppSettings, Value) {
        let tmp = Tmp::new(tag);
        let root = tmp.0.join("lib");
        std::fs::create_dir_all(&root).unwrap();
        let cache = json!({ "schemaVersion": 5, "libraryRoot": s(&root), "rigs": [], "assets": [{ "id": "uid://one", "user": { "favorite": true, "tags": ["rock"], "collections": ["Spygame"] } }] });
        std::fs::write(tmp.0.join("catalog.json"), serde_json::to_vec(&cache).unwrap()).unwrap();
        std::fs::create_dir_all(tmp.0.join("thumbs")).unwrap();
        std::fs::write(tmp.0.join("thumbs/uid___one.png"), b"png").unwrap();
        let mut settings: AppSettings = serde_json::from_value(json!({ "libraryRoot": s(&root), "animationPreviewVersion": 1 })).unwrap();
        migrate_settings(&mut settings, || "first".into());
        (tmp, settings, cache)
    }
    type Value = serde_json::Value;

    #[test]
    fn app_data_moves_to_the_first_library_and_personal_metadata_survives() {
        let (tmp, settings, cache) = legacy_fixture("move");
        migrate_app_data(&tmp.0, &settings).unwrap();
        let moved: Value = serde_json::from_slice(&std::fs::read(tmp.0.join("libraries/first/catalog.json")).unwrap()).unwrap();
        assert_eq!(moved, cache, "byte-for-byte the same document, favourites/tags/collections included");
        assert_eq!(moved["assets"][0]["user"]["collections"][0], "Spygame");
        assert_eq!(std::fs::read(tmp.0.join("libraries/first/thumbs/uid___one.png")).unwrap(), b"png");
        assert!(!tmp.0.join("catalog.json").exists() && !tmp.0.join("thumbs").exists());
        assert_eq!(marker(&tmp.0.join("libraries/first"), PREVIEW_MARKER), 1, "the global preview-refresh flag follows the library");
        // Running it again (a restart) is a no-op.
        migrate_app_data(&tmp.0, &settings).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(tmp.0.join("libraries/first/catalog.json")).unwrap()).unwrap(), cache);
    }

    #[test]
    fn app_data_migration_resumes_after_a_crash_at_any_step() {
        // Crash after the thumbnails moved but before the cache did.
        let (tmp, settings, cache) = legacy_fixture("crash_a");
        std::fs::create_dir_all(tmp.0.join("libraries/first")).unwrap();
        std::fs::rename(tmp.0.join("thumbs"), tmp.0.join("libraries/first/thumbs")).unwrap();
        migrate_app_data(&tmp.0, &settings).unwrap();
        assert!(tmp.0.join("libraries/first/thumbs/uid___one.png").exists());
        assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(tmp.0.join("libraries/first/catalog.json")).unwrap()).unwrap(), cache);
        // Crash after the cache moved but before the thumbnails (not produced by our order, but must not lose them).
        let (tmp, settings, _) = legacy_fixture("crash_b");
        std::fs::create_dir_all(tmp.0.join("libraries/first")).unwrap();
        std::fs::rename(tmp.0.join("catalog.json"), tmp.0.join("libraries/first/catalog.json")).unwrap();
        migrate_app_data(&tmp.0, &settings).unwrap();
        assert!(tmp.0.join("libraries/first/thumbs/uid___one.png").exists(), "single registered library still owns stray thumbs");
        // Crash before settings were saved: nothing registered, nothing moved.
        let (tmp, _, _) = legacy_fixture("crash_c");
        migrate_app_data(&tmp.0, &AppSettings::default()).unwrap();
        assert!(tmp.0.join("catalog.json").exists() && tmp.0.join("thumbs").exists());
    }

    #[test]
    fn app_data_migration_never_overwrites_existing_library_data() {
        let (tmp, settings, _) = legacy_fixture("keep");
        std::fs::create_dir_all(tmp.0.join("libraries/first")).unwrap();
        std::fs::write(tmp.0.join("libraries/first/catalog.json"), b"{\"newer\":true}").unwrap();
        migrate_app_data(&tmp.0, &settings).unwrap();
        assert_eq!(std::fs::read(tmp.0.join("libraries/first/catalog.json")).unwrap(), b"{\"newer\":true}");
        assert!(tmp.0.join("catalog.json.legacy").exists(), "the old file is kept aside, not deleted");
        migrate_app_data(&tmp.0, &settings).unwrap();
        assert!(tmp.0.join("catalog.json.legacy").exists());
    }

    #[test]
    fn a_removed_library_keeps_its_app_data_and_adding_the_folder_again_recognises_it() {
        let tmp = Tmp::new("readd");
        let root = tmp.0.join("lib");
        std::fs::create_dir_all(&root).unwrap();
        let mut settings = AppSettings::default();
        let entry = settings.add_library(&s(&root), None, "kept1".into()).unwrap();
        write_sidecar(&tmp.0, &entry).unwrap();
        let handle = LibraryHandle::local(&tmp.0, &entry);
        std::fs::write(&handle.cache_path, b"{\"favourites\":true}").unwrap();
        settings.remove_library("kept1").unwrap();
        // Removing touches neither the library folder nor its kept app data.
        assert!(root.is_dir() && handle.cache_path.exists());
        // The same folder, spelled differently, gets its old id (and so its metadata) back.
        let spelled = format!("{}{}", s(&root).replace('\\', "/"), "/");
        assert_eq!(retained_id_for(&tmp.0, &settings, &spelled).as_deref(), Some("kept1"));
        assert_eq!(retained_id_for(&tmp.0, &settings, &s(&tmp.0.join("elsewhere"))), None);
        // While registered, the id is not "retained" (it is simply in use).
        settings.add_library(&s(&root), None, "kept1".into()).unwrap();
        assert_eq!(retained_id_for(&tmp.0, &settings, &s(&root)), None);
    }
}
