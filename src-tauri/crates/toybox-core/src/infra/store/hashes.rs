//! Content hashes (`ops::content_hash`): `storage key -> hash`.
//!
//! The map is a `hashes` table, not a column of `assets`, because a hash belongs
//! to a storage folder: it exists before the catalog row that will show it, it
//! is shared by clips cut from one file and by variants in one folder, and it
//! stays while a folder is on disk after its item is gone (the behaviour the
//! JSON side cache had, `prune` included). `assets.hash_key` / `content_hash`
//! mirror it for the rows that exist, so a later step can index on it.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::{meta_get, meta_int, meta_set, open_existing, open_or_create, Kind};
use crate::error::AppResult;

/// The map in the database, or empty when it was written under another scheme.
pub(crate) fn load_map(connection: &Connection) -> AppResult<HashMap<String, String>> {
    let mut map = HashMap::new();
    if meta_get(connection, "hashScheme")?.and_then(|text| text.parse::<u32>().ok()) != Some(crate::ops::content_hash::SCHEME_VERSION) {
        return Ok(map);
    }
    let mut statement = connection.prepare("SELECT key, hash FROM hashes")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        map.insert(row.get(0)?, row.get(1)?);
    }
    Ok(map)
}

/// The hashes of the library database at `path`; empty if there is none.
pub fn load(path: &Path) -> HashMap<String, String> {
    open_existing(path, Kind::Library).ok().flatten().and_then(|connection| load_map(&connection).ok()).unwrap_or_default()
}

/// Whether the `hashes` table was written under the running hash scheme.
pub(crate) fn scheme_ok(connection: &Connection) -> AppResult<bool> {
    Ok(meta_get(connection, "hashScheme")?.and_then(|text| text.parse::<u32>().ok()) == Some(crate::ops::content_hash::SCHEME_VERSION))
}

/// What a change of hashes did.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Effect {
    /// The map changed (the hash revision goes up).
    pub(crate) touched: bool,
    /// Rows that show a changed hash were rewritten and logged in `changes`, so a
    /// client that follows the log gets them; the library revision goes up.
    pub(crate) rows: bool,
}

impl Effect {
    fn merge(&mut self, other: Effect) {
        self.touched |= other.touched;
        self.rows |= other.rows;
    }
}

/// Make the rows that show the hash of `key` (their own, or a variant's) say
/// `hash`, and log them as changed at `revision`. True if there were any.
fn touch_key(tx: &Connection, key: &str, hash: Option<&str>, revision: i64) -> AppResult<bool> {
    tx.prepare_cached("UPDATE assets SET content_hash=?2 WHERE hash_key=?1")?.execute(params![key, hash])?;
    let ids: Vec<String> = {
        let mut statement = tx.prepare_cached("SELECT id FROM assets WHERE hash_key=?1 UNION SELECT asset_id FROM asset_hash_keys WHERE hash_key=?1")?;
        let found = statement.query_map([key], |row| row.get(0))?;
        found.collect::<Result<_, _>>()?
    };
    tx.prepare_cached("UPDATE asset_hash_keys SET hashed=?2 WHERE hash_key=?1")?.execute(params![key, hash.is_some() as i64])?;
    for id in &ids {
        tx.prepare_cached("UPDATE assets SET revision=?2 WHERE id=?1")?.execute(params![id, revision])?;
        tx.prepare_cached("INSERT OR REPLACE INTO changes(revision,asset_id,deleted) VALUES(?1,?2,0)")?.execute(params![revision, id])?;
    }
    Ok(!ids.is_empty())
}

/// Another definition of the hash: nothing of the old one is kept, and every row
/// that showed one is logged as changed.
fn wipe_stale(tx: &Connection, revision: i64) -> AppResult<Effect> {
    let mut effect = Effect::default();
    if tx.execute("DELETE FROM hashes", [])? > 0 {
        tx.execute("UPDATE assets SET content_hash=NULL WHERE content_hash IS NOT NULL AND hash_key IS NOT NULL", [])?;
        let shown = "SELECT DISTINCT asset_id FROM asset_hash_keys WHERE hashed=1";
        effect.rows = tx.execute(&format!("INSERT OR REPLACE INTO changes(revision,asset_id,deleted) SELECT ?1, asset_id, 0 FROM ({shown})"), [revision])? > 0;
        tx.execute(&format!("UPDATE assets SET revision=?1 WHERE id IN ({shown})"), [revision])?;
        tx.execute("UPDATE asset_hash_keys SET hashed=0 WHERE hashed=1", [])?;
        effect.touched = true;
    }
    Ok(effect)
}

/// Bump the library revision when a standalone hash change touched rows.
fn finish(tx: &Connection, revision: i64, effect: Effect) -> AppResult<()> {
    if effect.touched {
        meta_set(tx, "hashRevision", &(meta_int(tx, "hashRevision")? + 1).to_string())?;
    }
    if effect.rows {
        meta_set(tx, "revision", &revision.to_string())?;
        super::changes::prune_in_tx(tx, revision)?;
    }
    Ok(())
}

/// Change the map in one transaction. Only the differing keys are written; the
/// hash revision goes up if there were any. The library revision goes up only
/// when rows that show a changed hash exist (they are logged for `/changes`).
pub fn update(path: &Path, change: impl FnOnce(&mut HashMap<String, String>)) -> AppResult<()> {
    let mut connection = open_or_create(path, Kind::Library)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let revision = meta_int(&tx, "revision")? + 1;
    let stale_scheme = !scheme_ok(&tx)?;
    let scheme = crate::ops::content_hash::SCHEME_VERSION.to_string();
    let before = load_map(&tx)?;
    let mut after = before.clone();
    change(&mut after);
    let mut effect = Effect::default();
    if stale_scheme {
        effect.merge(wipe_stale(&tx, revision)?);
        meta_set(&tx, "hashScheme", &scheme)?;
    }
    for key in before.keys().filter(|key| !after.contains_key(*key)) {
        tx.execute("DELETE FROM hashes WHERE key=?1", [key])?;
        effect.rows |= touch_key(&tx, key, None, revision)?;
        effect.touched = true;
    }
    for (key, hash) in &after {
        if before.get(key) != Some(hash) {
            tx.execute("INSERT INTO hashes(key,hash) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET hash=excluded.hash", params![key, hash])?;
            effect.rows |= touch_key(&tx, key, Some(hash), revision)?;
            effect.touched = true;
        }
    }
    finish(&tx, revision, effect)?;
    tx.commit()?;
    Ok(())
}

/// Record `entries` (storage key -> hash) inside an open transaction that commits
/// `revision`: only these keys are read or written, and the rows that show them
/// get the hash and are logged as changed. The caller bumps the revisions.
pub(crate) fn set_in_tx(tx: &Connection, entries: &[(String, String)], revision: i64) -> AppResult<Effect> {
    let mut effect = Effect::default();
    if entries.is_empty() {
        return Ok(effect);
    }
    if !scheme_ok(tx)? {
        effect.merge(wipe_stale(tx, revision)?);
        meta_set(tx, "hashScheme", &crate::ops::content_hash::SCHEME_VERSION.to_string())?;
    }
    let mut read = tx.prepare_cached("SELECT hash FROM hashes WHERE key=?1")?;
    let mut write = tx.prepare_cached("INSERT INTO hashes(key,hash) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET hash=excluded.hash")?;
    for (key, hash) in entries {
        let current: Option<String> = read.query_row([key], |row| row.get(0)).optional()?;
        if current.as_deref() != Some(hash.as_str()) {
            write.execute(params![key, hash])?;
            effect.rows |= touch_key(tx, key, Some(hash), revision)?;
            effect.touched = true;
        }
    }
    drop((read, write));
    Ok(effect)
}

/// Forget the hashes of these storage keys (and clear them on the rows that show them).
pub(crate) fn forget_in_tx(tx: &Connection, keys: &[String], revision: i64) -> AppResult<Effect> {
    let mut effect = Effect::default();
    for key in keys {
        if tx.execute("DELETE FROM hashes WHERE key=?1", [key])? > 0 {
            effect.rows |= touch_key(tx, key, None, revision)?;
            effect.touched = true;
        }
    }
    Ok(effect)
}

/// Set and forget a few keys in one transaction, without reading the rest of the
/// map. (`update` hands the whole map to a closure; this is its incremental form.)
pub fn update_keys(path: &Path, set: &[(String, String)], forget: &[String]) -> AppResult<()> {
    let mut connection = open_or_create(path, Kind::Library)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let revision = meta_int(&tx, "revision")? + 1;
    let mut effect = forget_in_tx(&tx, forget, revision)?;
    effect.merge(set_in_tx(&tx, set, revision)?);
    finish(&tx, revision, effect)?;
    tx.commit()?;
    Ok(())
}

/// The hashes of these storage keys that are stored.
pub fn get_keys(path: &Path, keys: &[String]) -> HashMap<String, String> {
    let mut found = HashMap::new();
    let Some(connection) = open_existing(path, Kind::Library).ok().flatten() else { return found };
    if meta_get(&connection, "hashScheme").ok().flatten().and_then(|text| text.parse::<u32>().ok()) != Some(crate::ops::content_hash::SCHEME_VERSION) {
        return found;
    }
    if let Ok(mut statement) = connection.prepare_cached("SELECT hash FROM hashes WHERE key=?1") {
        for key in keys {
            if let Ok(hash) = statement.query_row([key], |row| row.get::<_, String>(0)) {
                found.insert(key.clone(), hash);
            }
        }
    }
    found
}
