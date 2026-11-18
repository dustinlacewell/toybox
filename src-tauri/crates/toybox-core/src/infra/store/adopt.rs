//! Step 4 of the library migration (`sqlite-store`): the manifest, the cache and
//! the content-hash file of a library become databases.
//!
//! `library.db.new` is built from the documents, verified against them, and only
//! then renamed to `library.db`; afterwards the old files are moved (never
//! deleted) into `_library_config/migration-backups/0004-<stamp>/`. Every step
//! can be repeated after a crash: a leftover `library.db.new` is discarded and
//! rebuilt, a finished database is not rebuilt, and moving skips what is gone.
//! The manifest is moved last, because its presence is what says "not finished".

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{library_db, manifest, CatalogWrite, Kind, Store};
use crate::domain::catalog_migrate;
use crate::domain::catalog_model::{Asset, Catalog, ThumbMeta, ThumbState};
use crate::error::{AppError, AppResult};
use crate::infra::registry::{self, LibraryHandle};

const HASH_FILE: &str = "content-hashes.json";

fn remove_database_files(path: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(name));
    }
}

fn move_file(from: &Path, to: &Path) -> AppResult<()> {
    if !from.exists() {
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::file("Create directory", parent, e))?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // Another volume (app data and the library are often on different drives).
    std::fs::copy(from, to).map_err(|e| AppError::file("Copy", from, e))?;
    std::fs::remove_file(from).map_err(|e| AppError::file("Remove", from, e))
}

fn same_library(recorded: &str, root: &Path) -> bool {
    Path::new(recorded) == root || registry::same_root(recorded, &root.to_string_lossy())
}

fn unreadable(cache: &Path, error: impl std::fmt::Display) -> AppError {
    AppError::msg(format!("Migration stopped: the library cache '{}' cannot be read ({error}). Move it away to rebuild it.", cache.display()))
}

/// The cache file as a catalog, if it belongs to the library at `root`. An
/// unreadable cache is an error: it holds personal data that must not vanish.
fn read_cache(cache: &Path, root: &Path) -> AppResult<Option<Catalog>> {
    let Ok(bytes) = std::fs::read(cache) else { return Ok(None) };
    let doc: Value = serde_json::from_slice(&bytes).map_err(|e| unreadable(cache, e))?;
    if !doc["libraryRoot"].as_str().is_some_and(|recorded| same_library(recorded, root)) {
        return Ok(None);
    }
    let catalog = serde_json::from_value(catalog_migrate::upgrade(doc)).map_err(|e| unreadable(cache, e))?;
    Ok(Some(catalog))
}

fn read_hashes(path: &Path) -> HashMap<String, String> {
    let doc = std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    match doc {
        Some(doc) if doc["scheme"].as_u64() == Some(u64::from(crate::ops::content_hash::SCHEME_VERSION)) => {
            doc["hashes"].as_object().into_iter().flatten().filter_map(|(key, hash)| Some((key.clone(), hash.as_str()?.to_string()))).collect()
        }
        _ => HashMap::new(),
    }
}

/// A catalog as the databases keep it: thumbnail states other than an error
/// are not kept, and a local library derives content hashes.
fn comparable(catalog: &Catalog, keep_hash: bool) -> AppResult<Value> {
    fn normalise(asset: &mut Asset, keep_hash: bool) {
        if !matches!(asset.thumb.state, ThumbState::Error) && asset.thumb.error.is_none() {
            asset.thumb = ThumbMeta::default();
        }
        if !keep_hash {
            asset.content_hash = None;
        }
        for variant in &mut asset.variants {
            normalise(variant, keep_hash);
        }
    }
    let mut copy = catalog.clone();
    copy.library_root = String::new();
    for asset in &mut copy.assets {
        normalise(asset, keep_hash);
    }
    Ok(serde_json::to_value(copy)?)
}

fn build_and_verify(new: &Path, user: &Path, root: &Path, manifest_doc: &Value, cache: Option<&Catalog>, hashes: &HashMap<String, String>) -> AppResult<()> {
    remove_database_files(new);
    let store = Store { library: new.to_path_buf(), user: user.to_path_buf(), root: root.to_string_lossy().to_string(), remote: false };
    let built = (|| -> AppResult<()> {
        super::hashes::update(new, |map| *map = hashes.clone())?;
        store.commit(CatalogWrite { manifest: Some(manifest_doc), catalog: cache, expect_manifest_revision: None })?;
        // Verify from the file, as a reader would.
        let stored = manifest::read_at(new)?;
        if &stored != manifest_doc {
            return Err(AppError::msg("Migration check failed: the manifest in the new database differs from the original"));
        }
        let wanted = manifest_doc["assets"].as_array().map_or(0, Vec::len);
        if stored["assets"].as_array().map_or(0, Vec::len) != wanted {
            return Err(AppError::msg("Migration check failed: item counts differ"));
        }
        if let Some(cache) = cache {
            let back = store.load_catalog()?.ok_or_else(|| AppError::msg("Migration check failed: the catalog was not stored"))?;
            if back.assets.len() != cache.assets.len() || comparable(&back, false)? != comparable(cache, false)? {
                return Err(AppError::msg("Migration check failed: the catalog in the new database differs from the cache"));
            }
        }
        if super::hashes::load(new) != *hashes {
            return Err(AppError::msg("Migration check failed: the content hashes differ"));
        }
        Ok(())
    })();
    if let Err(error) = built {
        remove_database_files(new);
        return Err(error);
    }
    // Fold the write-ahead log into the file so the rename moves everything.
    let connection = super::open_existing(new, Kind::Library)?.ok_or_else(|| AppError::msg("Migration check failed: the new database vanished"))?;
    connection.query_row("PRAGMA journal_mode=DELETE", [], |row| row.get::<_, String>(0))?;
    drop(connection);
    Ok(())
}

/// Turn the JSON manifest, cache and content hashes of the local library at
/// `root` (cache file `cache`) into databases. Returns whether there was
/// anything to adopt. Safe to repeat.
pub fn adopt_local(root: &Path, cache: &Path) -> AppResult<bool> {
    let manifest_path = root.join(crate::config::SEED_REL);
    if !manifest_path.is_file() {
        return Ok(false);
    }
    let db = library_db(root);
    let finished = super::open_existing(&db, Kind::Library)?.is_some_and(|c| manifest::has_manifest(&c));
    if !finished {
        let manifest_doc: Value = serde_json::from_slice(&std::fs::read(&manifest_path).map_err(|e| AppError::file("Read", &manifest_path, e))?)?;
        if !manifest_doc["assets"].is_array() {
            return Err(AppError::msg("Cannot migrate library: catalog assets must be an array"));
        }
        let user = cache.parent().map(|dir| dir.join("user.db")).unwrap_or_else(|| PathBuf::from("user.db"));
        let hashes = read_hashes(&cache.with_file_name(HASH_FILE));
        let cache_catalog = read_cache(cache, root)?;
        let new = db.with_file_name("library.db.new");
        build_and_verify(&new, &user, root, &manifest_doc, cache_catalog.as_ref(), &hashes)?;
        remove_database_files(&db);
        std::fs::rename(&new, &db).map_err(|e| AppError::file("Install library database", &db, e))?;
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let backup = root.join("_library_config/migration-backups").join(format!("0004-{stamp}"));
    move_file(cache, &backup.join("cache.before.json"))?;
    move_file(&cache.with_file_name(HASH_FILE), &backup.join("content-hashes.before.json"))?;
    move_file(&manifest_path, &backup.join("manifest.before.json"))?;
    Ok(true)
}

/// A remote library's cache file from before the databases: its catalog (with
/// the personal data in it) becomes the replica. Nothing happens when there is
/// no such file; a replica that has a catalog already is kept.
pub fn adopt_remote_cache(handle: &LibraryHandle) -> AppResult<()> {
    if !handle.is_remote() || !handle.cache_path.is_file() {
        return Ok(());
    }
    let store = Store::for_handle(handle);
    if !store.catalog_exists() {
        let bytes = std::fs::read(&handle.cache_path).map_err(|e| AppError::file("Read", &handle.cache_path, e))?;
        let doc: Value = serde_json::from_slice(&bytes)?;
        let catalog: Catalog = serde_json::from_value(catalog_migrate::upgrade(doc))?;
        store.save_catalog(&catalog)?;
        let back = store.load_catalog()?.ok_or_else(|| AppError::msg("The cached catalog was not stored"))?;
        if comparable(&back, true)? != comparable(&catalog, true)? {
            return Err(AppError::msg("Migration check failed: the stored catalog differs from the cache"));
        }
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    move_file(&handle.cache_path, &handle.data_dir().join("migration-backups").join(format!("0004-{stamp}")).join("cache.before.json"))
}

/// Whether two catalog documents are the same catalog as the databases keep it
/// (thumbnail states other than errors, content hashes and the folder do not
/// count). Documents that do not parse compare as plain JSON.
pub fn same_catalog(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let parse = |doc: &Value| serde_json::from_value::<Catalog>(catalog_migrate::upgrade(doc.clone())).ok().and_then(|catalog| comparable(&catalog, false).ok());
            match (parse(a), parse(b)) {
                (Some(a), Some(b)) => a == b,
                _ => a == b,
            }
        }
        _ => false,
    }
}
