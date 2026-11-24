//! SQLite storage of a library (see `docs/sqlite-storage.md`).
//!
//! Two databases per library: `library.db` (the manifest as `items`, the derived
//! catalog rows, content hashes, rigs and the revision log) and `user.db` (this
//! computer's personal data). Everything above this module still talks in whole
//! documents (`Value` manifest, `Catalog`), so this is a compatibility layer:
//! each call diffs the document against the rows and writes only what changed.
//!
//! * [`manifest`]: `read_manifest` / `write_manifest` and the revision stamp.
//! * [`catalog`]: `Store::save_catalog` / `load_catalog`, the split of an
//!   `Asset` into library data and personal data.
//! * [`hashes`]: the content-hash side cache.
//! * [`adopt`]: step 4 of the library migration, JSON files into databases.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::infra::registry::LibraryHandle;

pub mod adopt;
pub mod bench_data;
pub mod browse;
pub mod browse_index;
pub mod catalog;
pub mod changes;
pub mod delta;
pub mod hashes;
pub mod manifest;
pub mod query;
#[cfg(test)]
mod browse_tests;
#[cfg(test)]
mod browse_index_tests;
#[cfg(test)]
mod changes_tests;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
pub(crate) mod testing;

pub use adopt::same_catalog;
pub use catalog::{CatalogWrite, Committed};
pub use changes::{ChangesPage, ChangesQuery, CHANGES_KEEP_REVISIONS};
pub use delta::{BulkWriter, Delta, ItemPut};
pub use manifest::{manifest_exists, manifest_revision, read_manifest, read_manifest_with_revision, write_manifest, write_manifest_checked};

/// Schema version of `library.db`. Version 2 added `assets.derived_from` (see `upgrade_library`).
pub const SCHEMA_VERSION: i64 = 2;
/// Schema version of `user.db`.
pub const USER_SCHEMA_VERSION: i64 = 1;
const BUSY_MS: u64 = 15_000;

const LIBRARY_SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE items (
  id            TEXT PRIMARY KEY,
  seq           INTEGER NOT NULL UNIQUE,
  kind          TEXT NOT NULL,
  type          TEXT NOT NULL,
  pack          TEXT NOT NULL,
  category      TEXT NOT NULL,
  name          TEXT NOT NULL,
  storage_path  TEXT,
  entry         TEXT NOT NULL,
  revision      INTEGER NOT NULL,
  pkey          TEXT
);
CREATE INDEX items_pkey ON items(pkey);
CREATE INDEX items_storage ON items(storage_path);
CREATE TABLE item_variants (
  variant_id TEXT NOT NULL,
  item_id    TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  thumb_name TEXT NOT NULL,
  PRIMARY KEY (variant_id, item_id)
) WITHOUT ROWID;
CREATE INDEX item_variants_item ON item_variants(item_id);
CREATE INDEX item_variants_thumb ON item_variants(thumb_name);
CREATE TABLE assets (
  id            TEXT PRIMARY KEY,
  item_id       TEXT,
  ord           INTEGER NOT NULL,
  kind          TEXT NOT NULL,
  type          TEXT NOT NULL,
  name          TEXT NOT NULL,
  pack          TEXT NOT NULL,
  category      TEXT NOT NULL,
  format        TEXT NOT NULL DEFAULT '',
  source        TEXT NOT NULL DEFAULT '',
  publisher     TEXT NOT NULL DEFAULT '',
  license       TEXT NOT NULL DEFAULT '',
  content_hash  TEXT,
  hash_key      TEXT,
  asset         TEXT NOT NULL,
  revision      INTEGER NOT NULL,
  rel_path      TEXT NOT NULL DEFAULT '',
  thumb_name    TEXT NOT NULL DEFAULT '',
  clip_source   TEXT NOT NULL DEFAULT '',
  plugin        TEXT NOT NULL DEFAULT '',
  derived_from  TEXT NOT NULL DEFAULT ''
);
CREATE INDEX assets_item ON assets(item_id, ord);
CREATE INDEX assets_rel ON assets(rel_path);
CREATE INDEX assets_thumb ON assets(thumb_name);
CREATE INDEX assets_clip_source ON assets(clip_source);
CREATE INDEX assets_derived_from ON assets(derived_from);
CREATE INDEX assets_order ON assets(ord);
CREATE INDEX assets_browse ON assets(type, name COLLATE NOCASE, id, content_hash);
CREATE INDEX assets_hash ON assets(type, content_hash);
CREATE INDEX assets_unhashed ON assets(type, id) WHERE content_hash IS NULL;
CREATE INDEX assets_category ON assets(type, category, name COLLATE NOCASE, id);
CREATE INDEX assets_source ON assets(type, source);
CREATE INDEX assets_publisher ON assets(type, publisher);
CREATE INDEX assets_license ON assets(type, license);
CREATE INDEX assets_format ON assets(type, format);
CREATE INDEX assets_hash_key ON assets(hash_key);
CREATE INDEX assets_revision ON assets(revision);
CREATE TABLE asset_tags (
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  tag      TEXT NOT NULL,
  type     TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (asset_id, tag)
) WITHOUT ROWID;
CREATE INDEX asset_tags_tag ON asset_tags(tag, asset_id);
CREATE VIRTUAL TABLE asset_search USING fts5(name, tags, content='', contentless_delete=1, tokenize='trigram');
CREATE TABLE asset_files (
  path     TEXT NOT NULL COLLATE NOCASE,
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  origin   INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (path, asset_id)
) WITHOUT ROWID;
CREATE INDEX asset_files_asset ON asset_files(asset_id);
CREATE TABLE rigs (id TEXT PRIMARY KEY, rig TEXT NOT NULL, kind TEXT NOT NULL DEFAULT '', shape TEXT NOT NULL DEFAULT '');
CREATE INDEX rigs_shape ON rigs(shape);
CREATE TABLE rig_uses (
  rig_id   TEXT NOT NULL,
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  PRIMARY KEY (rig_id, asset_id)
) WITHOUT ROWID;
CREATE INDEX rig_uses_asset ON rig_uses(asset_id);
CREATE TABLE changes (
  revision INTEGER NOT NULL,
  asset_id TEXT NOT NULL,
  deleted  INTEGER NOT NULL,
  PRIMARY KEY (asset_id)
) WITHOUT ROWID;
CREATE INDEX changes_revision ON changes(revision, deleted, asset_id);
CREATE INDEX changes_tombstones ON changes(revision) WHERE deleted = 1;
CREATE TABLE asset_hash_keys (
  hash_key TEXT NOT NULL,
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  hashed   INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (hash_key, asset_id)
) WITHOUT ROWID;
CREATE INDEX asset_hash_keys_asset ON asset_hash_keys(asset_id);
CREATE INDEX asset_hash_keys_pending ON asset_hash_keys(asset_id) WHERE hashed = 0;
CREATE TABLE thumbs (
  asset_id TEXT PRIMARY KEY,
  revision INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX thumbs_revision ON thumbs(revision);
CREATE TABLE hashes (key TEXT PRIMARY KEY, hash TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE facets (
  type  TEXT NOT NULL,
  field TEXT NOT NULL,
  value TEXT NOT NULL,
  n     INTEGER NOT NULL,
  PRIMARY KEY (type, field, value)
) WITHOUT ROWID;
CREATE INDEX facets_top ON facets(type, field, n DESC, value);
CREATE TRIGGER assets_facets_insert AFTER INSERT ON assets BEGIN
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'type','',1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'category',NEW.category,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'source',NEW.source,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'publisher',NEW.publisher,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'license',NEW.license,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'format',NEW.format,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
END;
CREATE TRIGGER assets_facets_delete AFTER DELETE ON assets BEGIN
  UPDATE facets SET n=n-1 WHERE type=OLD.type AND ((field='type' AND value='') OR (field='category' AND value=OLD.category) OR (field='source' AND value=OLD.source) OR (field='publisher' AND value=OLD.publisher) OR (field='license' AND value=OLD.license) OR (field='format' AND value=OLD.format));
  DELETE FROM facets WHERE type=OLD.type AND n<=0 AND ((field='type' AND value='') OR (field='category' AND value=OLD.category) OR (field='source' AND value=OLD.source) OR (field='publisher' AND value=OLD.publisher) OR (field='license' AND value=OLD.license) OR (field='format' AND value=OLD.format));
END;
CREATE TRIGGER assets_facets_update_type AFTER UPDATE OF type ON assets WHEN OLD.type IS NOT NEW.type BEGIN
  UPDATE facets SET n=n-1 WHERE type=OLD.type AND field='type' AND value='';
  DELETE FROM facets WHERE type=OLD.type AND field='type' AND value='' AND n<=0;
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'type','',1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
END;
CREATE TRIGGER assets_facets_update_category AFTER UPDATE OF category ON assets WHEN OLD.category IS NOT NEW.category BEGIN
  UPDATE facets SET n=n-1 WHERE type=OLD.type AND field='category' AND value=OLD.category;
  DELETE FROM facets WHERE type=OLD.type AND field='category' AND value=OLD.category AND n<=0;
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'category',NEW.category,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
END;
CREATE TRIGGER assets_facets_update_source AFTER UPDATE OF source ON assets WHEN OLD.source IS NOT NEW.source BEGIN
  UPDATE facets SET n=n-1 WHERE type=OLD.type AND field='source' AND value=OLD.source;
  DELETE FROM facets WHERE type=OLD.type AND field='source' AND value=OLD.source AND n<=0;
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'source',NEW.source,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
END;
CREATE TRIGGER assets_facets_update_publisher AFTER UPDATE OF publisher ON assets WHEN OLD.publisher IS NOT NEW.publisher BEGIN
  UPDATE facets SET n=n-1 WHERE type=OLD.type AND field='publisher' AND value=OLD.publisher;
  DELETE FROM facets WHERE type=OLD.type AND field='publisher' AND value=OLD.publisher AND n<=0;
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'publisher',NEW.publisher,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
END;
CREATE TRIGGER assets_facets_update_license AFTER UPDATE OF license ON assets WHEN OLD.license IS NOT NEW.license BEGIN
  UPDATE facets SET n=n-1 WHERE type=OLD.type AND field='license' AND value=OLD.license;
  DELETE FROM facets WHERE type=OLD.type AND field='license' AND value=OLD.license AND n<=0;
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'license',NEW.license,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
END;
CREATE TRIGGER assets_facets_update_format AFTER UPDATE OF format ON assets WHEN OLD.format IS NOT NEW.format BEGIN
  UPDATE facets SET n=n-1 WHERE type=OLD.type AND field='format' AND value=OLD.format;
  DELETE FROM facets WHERE type=OLD.type AND field='format' AND value=OLD.format AND n<=0;
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'format',NEW.format,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
END;
CREATE TRIGGER asset_tags_facets_insert AFTER INSERT ON asset_tags BEGIN
  INSERT INTO facets(type,field,value,n) VALUES(NEW.type,'tag',NEW.tag,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
END;
CREATE TRIGGER asset_tags_facets_delete AFTER DELETE ON asset_tags BEGIN
  UPDATE facets SET n=n-1 WHERE type=OLD.type AND field='tag' AND value=OLD.tag;
  DELETE FROM facets WHERE type=OLD.type AND field='tag' AND value=OLD.tag AND n<=0;
END;
";

/// What a bulk download into a replica leaves out until its end: the index kinds of statement of the library schema
/// that are not needed to write rows (most secondary indexes, the counts triggers, the search table).
pub(crate) struct Deferred {
    /// `index`, `trigger` or `fts`.
    pub kind: &'static str,
    pub name: String,
    pub sql: String,
}

/// Indexes the writing of a replica row itself looks things up in: kept during a bulk download.
const KEPT_IN_BULK: [&str; 5] = ["assets_item", "assets_rel", "asset_files_asset", "rig_uses_asset", "asset_hash_keys_asset"];

/// The deferrable statements of [`LIBRARY_SCHEMA`], read from it so there is one text of the schema.
pub(crate) fn deferred_schema() -> Vec<Deferred> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in LIBRARY_SCHEMA.lines() {
        if current.is_empty() && !line.starts_with("CREATE ") {
            continue;
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
        let in_trigger = current.starts_with("CREATE TRIGGER");
        if (in_trigger && line.trim() == "END;") || (!in_trigger && line.trim_end().ends_with(';')) {
            let statement = std::mem::take(&mut current);
            let words: Vec<&str> = statement.split_whitespace().collect();
            match (words.get(1).copied(), words.get(2).copied()) {
                (Some("INDEX"), Some(name)) if !KEPT_IN_BULK.contains(&name) => out.push(Deferred { kind: "index", name: name.to_string(), sql: statement }),
                (Some("TRIGGER"), Some(name)) => out.push(Deferred { kind: "trigger", name: name.to_string(), sql: statement }),
                (Some("VIRTUAL"), Some("TABLE")) => out.push(Deferred { kind: "fts", name: words[3].to_string(), sql: statement }),
                _ => {}
            }
        }
    }
    out
}

const USER_SCHEMA: &str = "
CREATE TABLE user_assets (
  asset_id        TEXT PRIMARY KEY,
  favorite        INTEGER NOT NULL DEFAULT 0,
  preview_asset   TEXT,
  filesystem_path TEXT,
  thumb_error     TEXT,
  provenance      TEXT
);
CREATE TABLE user_tags (asset_id TEXT NOT NULL, tag TEXT NOT NULL, ord INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (asset_id, tag)) WITHOUT ROWID;
CREATE TABLE user_collections (asset_id TEXT NOT NULL, name TEXT NOT NULL, ord INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (asset_id, name)) WITHOUT ROWID;
CREATE INDEX user_collections_name ON user_collections(name, asset_id);
CREATE INDEX user_tags_tag ON user_tags(tag, asset_id);
CREATE INDEX user_assets_favorite ON user_assets(asset_id) WHERE favorite = 1;
CREATE INDEX user_assets_preview ON user_assets(preview_asset);
";

/// Which database a connection opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Library,
    User,
}

fn db_error(operation: &'static str, path: &Path, error: rusqlite::Error) -> AppError {
    AppError::file(operation, path, std::io::Error::other(error.to_string()))
}

fn configure(connection: &Connection, path: &Path) -> AppResult<()> {
    connection.busy_timeout(Duration::from_millis(BUSY_MS))?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
    let mode: String = connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0)).map_err(|e| db_error("Configure database", path, e))?;
    let _ = mode;
    Ok(())
}

/// Open a database, creating it (and its folder) with the current schema when absent.
pub(crate) fn open_or_create(path: &Path, kind: Kind) -> AppResult<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::file("Create directory", parent, e))?;
    }
    let connection = Connection::open(path).map_err(|e| db_error("Open database", path, e))?;
    configure(&connection, path)?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > schema_version(kind) {
        return Err(AppError::msg(format!("{} was written by a newer Toybox. Install that version or newer.", path.display())));
    }
    if version != 0 && version < schema_version(kind) {
        upgrade(&connection, kind)?;
    }
    if version == 0 {
        // Two openers racing to create it: the second sees the first's tables.
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let again: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if again == 0 {
            let schema = if kind == Kind::Library { LIBRARY_SCHEMA } else { USER_SCHEMA };
            let result = connection.execute_batch(schema).and_then(|_| {
                if kind == Kind::Library {
                    // An identity for this database, so a revision number alone never
                    // vouches for the content of another one (an ETag, for instance).
                    connection.execute("INSERT INTO meta(key,value) VALUES('dbId',?1)", [crate::infra::registry::new_id()])?;
                }
                connection.execute_batch(&format!("PRAGMA user_version={}", schema_version(kind)))
            });
            if let Err(error) = result {
                let _ = connection.execute_batch("ROLLBACK");
                return Err(error.into());
            }
        }
        connection.execute_batch("COMMIT")?;
    }
    Ok(connection)
}

/// Open an existing database; `None` when the file is not there. Nothing is created.
pub(crate) fn open_existing(path: &Path, kind: Kind) -> AppResult<Option<Connection>> {
    if !path.is_file() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|e| db_error("Open database", path, e))?;
    configure(&connection, path)?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > schema_version(kind) {
        return Err(AppError::msg(format!("{} was written by a newer Toybox. Install that version or newer.", path.display())));
    }
    if version == 0 {
        // An empty file left by an interrupted creation: not a database yet.
        return Ok(None);
    }
    if version < schema_version(kind) {
        upgrade(&connection, kind)?;
    }
    Ok(Some(connection))
}

fn schema_version(kind: Kind) -> i64 {
    match kind {
        Kind::Library => SCHEMA_VERSION,
        Kind::User => USER_SCHEMA_VERSION,
    }
}

/// Bring a database of an older schema up to the current one, in place, in one transaction (a
/// second opener racing for it waits for the lock and then finds it done). Every step only adds
/// what is derived from rows that are already there, so nothing is lost and a step can repeat.
fn upgrade(connection: &Connection, kind: Kind) -> AppResult<()> {
    if kind != Kind::Library {
        return Ok(());
    }
    connection.execute_batch("BEGIN IMMEDIATE")?;
    match upgrade_library(connection) {
        Ok(()) => Ok(connection.execute_batch("COMMIT")?),
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

/// The schema steps of `library.db`, from the version it has to [`SCHEMA_VERSION`].
fn upgrade_library(connection: &Connection) -> AppResult<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 2 {
        // 2: `assets.derived_from`, the indexed copy of `origin.derivedFrom`. It is filled from the
        // stored asset JSON (which always held the whole origin), so a library that already has such
        // links lists its derived assets without a rescan.
        let has: i64 = connection.query_row("SELECT COUNT(*) FROM pragma_table_info('assets') WHERE name='derived_from'", [], |row| row.get(0))?;
        if has == 0 {
            connection.execute_batch("ALTER TABLE assets ADD COLUMN derived_from TEXT NOT NULL DEFAULT ''")?;
        }
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS assets_derived_from ON assets(derived_from);
             UPDATE assets SET derived_from = json_extract(asset, '$.origin.derivedFrom')
               WHERE json_type(asset, '$.origin.derivedFrom') = 'text';",
        )?;
    }
    connection.execute_batch(&format!("PRAGMA user_version={SCHEMA_VERSION}"))?;
    Ok(())
}

pub(crate) fn meta_get(connection: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(connection.query_row("SELECT value FROM meta WHERE key=?1", [key], |row| row.get(0)).optional()?)
}

/// Set a meta value; returns whether it changed.
pub(crate) fn meta_set(connection: &Connection, key: &str, value: &str) -> AppResult<bool> {
    if meta_get(connection, key)?.as_deref() == Some(value) {
        return Ok(false);
    }
    connection.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [key, value])?;
    Ok(true)
}

pub(crate) fn meta_delete(connection: &Connection, key: &str) -> AppResult<bool> {
    Ok(connection.execute("DELETE FROM meta WHERE key=?1", [key])? > 0)
}

pub(crate) fn meta_int(connection: &Connection, key: &str) -> AppResult<i64> {
    Ok(meta_get(connection, key)?.and_then(|text| text.parse().ok()).unwrap_or(0))
}

/// What a caller can compare to learn that something it read has changed: the
/// revision goes up with every manifest or catalog write, the hash revision with
/// every change of content hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stamp {
    pub revision: i64,
    pub hash_revision: i64,
}

/// Where a library stands: its identity and the counters clients compare.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    pub db_id: String,
    /// Library revision: one per committed change (`GET /changes` is keyed by it).
    pub revision: i64,
    pub hash_revision: i64,
    /// Revision of the shared-thumbnail log, which is not part of `revision`.
    pub thumb_revision: i64,
}

/// The databases of one library.
#[derive(Debug, Clone)]
pub struct Store {
    pub(crate) library: PathBuf,
    pub(crate) user: PathBuf,
    /// Reported as `library_root` of a loaded catalog. Never stored: a library
    /// folder can be opened from different paths.
    pub(crate) root: String,
    /// A remote library's replica keeps what the server sent, content hashes included.
    pub(crate) remote: bool,
}

impl Store {
    pub fn for_handle(handle: &LibraryHandle) -> Store {
        Store { library: handle.library_db_path(), user: handle.user_db_path(), root: handle.root_str(), remote: handle.is_remote() }
    }

    /// The store of a local library given as a root and its cache path (the
    /// pair every older entry point takes). `user.db` sits where the cache was.
    pub fn local(root: &Path, cache: &Path) -> Store {
        Store {
            library: root.join(crate::config::LIBRARY_DB_REL),
            user: cache.parent().map(|dir| dir.join("user.db")).unwrap_or_else(|| PathBuf::from("user.db")),
            root: root.to_string_lossy().to_string(),
            remote: false,
        }
    }

    /// The replica of a remote library whose old cache file is `cache`.
    pub fn remote_at(root: &Path, cache: &Path) -> Store {
        let dir = cache.parent().map(Path::to_path_buf).unwrap_or_default();
        Store { library: dir.join("library.db"), user: dir.join("user.db"), root: root.to_string_lossy().to_string(), remote: true }
    }

    pub fn library_path(&self) -> &Path {
        &self.library
    }
    pub fn user_path(&self) -> &Path {
        &self.user
    }

    /// The change stamp, or the default when no database exists yet.
    pub fn stamp(&self) -> AppResult<Stamp> {
        let Some(connection) = open_existing(&self.library, Kind::Library)? else { return Ok(Stamp::default()) };
        Ok(Stamp { revision: meta_int(&connection, "revision")?, hash_revision: meta_int(&connection, "hashRevision")? })
    }

    /// Everything a server tells a client about where the library stands, read
    /// in one go. All zeros and an empty id when there is no database yet.
    pub fn status(&self) -> AppResult<Status> {
        let Some(connection) = open_existing(&self.library, Kind::Library)? else { return Ok(Status::default()) };
        Ok(Status {
            db_id: meta_get(&connection, "dbId")?.unwrap_or_default(),
            revision: meta_int(&connection, "revision")?,
            hash_revision: meta_int(&connection, "hashRevision")?,
            thumb_revision: meta_int(&connection, "thumbRevision")?,
        })
    }

    /// The identity of the library database (empty when there is none yet).
    pub fn db_id(&self) -> String {
        open_existing(&self.library, Kind::Library).ok().flatten().and_then(|c| meta_get(&c, "dbId").ok().flatten()).unwrap_or_default()
    }

    /// Whether a catalog (derived rows) has been saved.
    pub fn catalog_exists(&self) -> bool {
        open_existing(&self.library, Kind::Library).ok().flatten().and_then(|c| meta_get(&c, "catalogSchema").ok().flatten()).is_some()
    }

    /// Whether the derived rows were saved against the manifest as it is now.
    pub fn catalog_is_current(&self) -> AppResult<bool> {
        let Some(connection) = open_existing(&self.library, Kind::Library)? else { return Ok(false) };
        let Some(_) = meta_get(&connection, "catalogSchema")? else { return Ok(false) };
        Ok(meta_get(&connection, "derivedFor")? == Some(meta_int(&connection, "manifestRevision")?.to_string()))
    }
}

/// The library database of a local library at `root`.
pub fn library_db(root: &Path) -> PathBuf {
    root.join(crate::config::LIBRARY_DB_REL)
}

/// Whether a remote library whose app-data folder is `dir` has a catalog cached
/// (in its replica database, or in the cache file of an older version that the
/// next open turns into one).
pub fn remote_cache_present(dir: &Path) -> bool {
    dir.join("catalog.json").is_file() || Store { library: dir.join("library.db"), user: dir.join("user.db"), root: String::new(), remote: true }.catalog_exists()
}
