//! Incremental writes: a change of some items, applied to the rows of those items
//! only, in one transaction on `library.db` (plus the one on `user.db` opened
//! before it). The cost of [`Store::apply`] depends on the items in the
//! [`Delta`], never on how many others the library holds.
//!
//! What the caller hands over is already derived: the manifest entry of each
//! item and the catalog assets built from it (with their personal data, merged
//! the way a rescan would merge it). The store checks nothing about files.
//!
//! The same rows are written by the whole-document functions
//! (`manifest::write_manifest`, `Store::save_catalog`); a library written one way
//! reads back identical when written the other way (see `tests_delta`).

use std::collections::{BTreeMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::catalog::{self, Committed, UserRow};
use super::manifest::{self, ItemColumns};
use super::{hashes, meta_get, meta_int, meta_set, open_or_create, Kind, Store};
use crate::domain::catalog_model::Asset;
use crate::domain::rig::Rig;
use crate::error::{AppError, AppResult};

/// One manifest item and the catalog assets derived from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemPut {
    /// The manifest item the assets belong to.
    pub id: String,
    /// The item's entry as it is to be stored. `None` leaves the item row alone and
    /// only writes `assets` (a personal-data or tag patch); stale rows are then kept.
    pub entry: Option<Value>,
    /// The item's complete derived rows (with `user`), in order.
    pub assets: Vec<Asset>,
    /// Rigs the assets bind to; ones that are stored already are left as they are.
    #[serde(default)]
    pub rigs: Vec<Rig>,
}

/// A change of items. Removals happen first, so an item can be removed and put
/// back in one delta (it then goes to the end of the order).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Delta {
    pub puts: Vec<ItemPut>,
    /// Manifest items to remove with all their derived rows.
    pub remove_items: Vec<String>,
    /// Ids (assets or variants) that other assets may have as their preview choice;
    /// that choice is cleared.
    #[serde(default)]
    pub clear_previews: Vec<String>,
    /// Content hashes found for storage keys (`hashes` table and the rows that show them).
    #[serde(default)]
    pub hashes: Vec<(String, String)>,
    /// Set `storageVersion` to this when it is not that already.
    #[serde(default)]
    pub storage_version: Option<u64>,
    /// Write items and remove them, but leave the derived rows (and the "catalog is
    /// current" marks) alone: for finishing an interrupted publication in a library
    /// whose catalog is out of date anyway.
    #[serde(default)]
    pub skip_catalog: bool,
    /// Bookkeeping values to set in the same transaction (a replica's sync
    /// cursor, say). They never move the revision.
    #[serde(default)]
    pub set_meta: Vec<(String, String)>,
    /// Part of a bulk download into a replica that is in bulk mode (`Store::begin_bulk`): the search rows, counts and
    /// most indexes are not there and are made at the end, so they are not kept up with.
    #[serde(default)]
    pub bulk: bool,
    /// Refuse unless the manifest is still at this revision.
    #[serde(skip)]
    pub expect_manifest_revision: Option<i64>,
}

/// What an applied delta did.
#[derive(Debug, Clone)]
pub struct Applied {
    pub committed: Committed,
    /// Ids of the assets and variants that left the catalog.
    pub removed_ids: Vec<String>,
}

fn variant_ids_of(blob: &Value, out: &mut Vec<String>) {
    for variant in blob.get("variants").and_then(Value::as_array).into_iter().flatten() {
        if let Some(id) = variant.get("id").and_then(Value::as_str) {
            out.push(id.to_string());
        }
        variant_ids_of(variant, out);
    }
}

impl Store {
    /// Whether the saved catalog can take an incremental write: it was derived for
    /// the manifest as it is, under the running schema. A library with nothing in
    /// it needs no catalog yet. Anything else needs a rescan first.
    pub fn catalog_ready(&self) -> bool {
        let Ok(Some(connection)) = super::open_existing(&self.library, Kind::Library) else { return true };
        ready(&connection).unwrap_or(false)
    }

    /// Apply `delta`. One revision for the whole change; only the rows of the
    /// items in it are read or written.
    pub fn apply(&self, delta: &Delta) -> AppResult<Applied> {
        // A replica removes asset rows (there are no manifest items to remove) but never holds entries.
        if self.remote && delta.puts.iter().any(|put| put.entry.is_some()) {
            return Err(AppError::msg("A remote library has no manifest here"));
        }
        let mut library = open_or_create(&self.library, Kind::Library)?;
        let mut user = open_or_create(&self.user, Kind::User)?;
        self.apply_on(&mut library, &mut user, delta)
    }

    /// A writer for a bulk download: the connections stay open from page to page, so the pages of the big b-trees that the
    /// rows go into stay in the cache instead of being read again for each page.
    pub fn bulk_writer(&self) -> AppResult<BulkWriter> {
        let library = open_or_create(&self.library, Kind::Library)?;
        // Rows go into big b-trees at random places: a large cache, and the file mapped instead of read.
        library.execute_batch("PRAGMA cache_size=-524288; PRAGMA mmap_size=2147483648; PRAGMA wal_autocheckpoint=100000;")?;
        let user = open_or_create(&self.user, Kind::User)?;
        Ok(BulkWriter { store: self.clone(), library, user })
    }

    fn apply_on(&self, library: &mut Connection, user: &mut Connection, delta: &Delta) -> AppResult<Applied> {
        let user_tx = user.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let tx = library.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(expected) = delta.expect_manifest_revision {
            if meta_int(&tx, "manifestRevision")? != expected {
                return Err(AppError::msg("The library changed while it was being edited. Try again."));
            }
        }
        // Removing items is valid whatever state the derived rows are in; writing derived
        // rows needs them to be current (a rescan brings them there).
        let was_ready = self.remote || ready(&tx)?;
        if !was_ready && !delta.skip_catalog && delta.puts.iter().any(|put| !put.assets.is_empty()) {
            return Err(AppError::msg("The library's catalog is not current. Rescan the library first."));
        }
        let previous = meta_int(&tx, "revision")?;
        let revision = previous + 1;
        let user_empty = delta.bulk
            && ["user_assets", "user_tags", "user_collections"].iter().all(|table| user_tx.query_row(&format!("SELECT NOT EXISTS (SELECT 1 FROM {table})"), [], |row| row.get::<_, bool>(0)).unwrap_or(false));
        let mut run = Run {
            tx: &tx,
            user_tx: &user_tx,
            user_empty,
            revision,
            remote: self.remote,
            skip_catalog: delta.skip_catalog,
            bulk: delta.bulk,
            next_seq: tx.query_row("SELECT COALESCE(MAX(seq), -1) + 1 FROM items", [], |row| row.get(0))?,
            next_ord: tx.query_row("SELECT COALESCE(MAX(ord), -1) + 1 FROM assets", [], |row| row.get(0))?,
            hash_ok: meta_get(&tx, "hashScheme")?.and_then(|text| text.parse::<u32>().ok()) == Some(crate::ops::content_hash::SCHEME_VERSION),
            manifest_changed: false,
            changed: false,
            removed_ids: Vec::new(),
            rig_candidates: HashSet::new(),
        };

        for id in &delta.remove_items {
            run.remove_item(id)?;
        }
        run.clear_previews(&delta.clear_previews)?;
        for put in &delta.puts {
            run.put(put)?;
        }
        // Rigs that lost their last user go.
        for rig in std::mem::take(&mut run.rig_candidates) {
            tx.execute("DELETE FROM rigs WHERE id=?1 AND NOT EXISTS (SELECT 1 FROM rig_uses WHERE rig_id=?1)", [&rig])?;
        }
        if !delta.hashes.is_empty() {
            let effect = hashes::set_in_tx(&tx, &delta.hashes, revision)?;
            if effect.touched {
                meta_set(&tx, "hashRevision", &(meta_int(&tx, "hashRevision")? + 1).to_string())?;
                run.changed = true;
            }
        }
        for (key, value) in &delta.set_meta {
            meta_set(&tx, key, value)?;
        }
        if let Some(version) = delta.storage_version {
            if meta_set(&tx, "storageVersion", &version.to_string())? {
                run.manifest_changed = true;
                run.changed = true;
            }
        }
        if run.manifest_changed {
            meta_set(&tx, "manifestRevision", &revision.to_string())?;
        }
        if !self.remote && !delta.skip_catalog && was_ready {
            meta_set(&tx, "catalogSchema", &crate::config::SCHEMA_VERSION.to_string())?;
            meta_set(&tx, "derivedFor", &meta_int(&tx, "manifestRevision")?.to_string())?;
        }
        let changed = run.changed;
        let removed_ids = run.removed_ids;
        if changed {
            meta_set(&tx, "revision", &revision.to_string())?;
            if !delta.bulk {
                super::changes::prune_in_tx(&tx, revision)?;
            }
        }
        tx.commit()?;
        user_tx.commit()?;
        Ok(Applied { committed: Committed { revision: if changed { revision } else { previous }, changed }, removed_ids })
    }
}

/// The connections of a bulk download (see [`Store::bulk_writer`]).
pub struct BulkWriter {
    store: Store,
    library: Connection,
    user: Connection,
}

impl BulkWriter {
    /// [`Store::apply`] on the connections held. `delta.bulk` should be set.
    pub fn apply(&mut self, delta: &Delta) -> AppResult<Applied> {
        if self.store.remote && delta.puts.iter().any(|put| put.entry.is_some()) {
            return Err(AppError::msg("A remote library has no manifest here"));
        }
        self.store.apply_on(&mut self.library, &mut self.user, delta)
    }
}

fn ready(connection: &Connection) -> AppResult<bool> {
    match meta_get(connection, "catalogSchema")? {
        Some(schema) => Ok(schema.parse::<u32>().ok() == Some(crate::config::SCHEMA_VERSION)
            && meta_get(connection, "derivedFor")? == Some(meta_int(connection, "manifestRevision")?.to_string())),
        None => {
            let items: i64 = connection.query_row("SELECT COUNT(*) FROM (SELECT 1 FROM items LIMIT 1)", [], |row| row.get(0))?;
            Ok(items == 0)
        }
    }
}

struct Run<'a> {
    tx: &'a Transaction<'a>,
    user_tx: &'a Transaction<'a>,
    /// `user.db` holds no rows at all (known only for a bulk download): there is nothing to look for in it.
    user_empty: bool,
    revision: i64,
    remote: bool,
    skip_catalog: bool,
    bulk: bool,
    next_seq: i64,
    next_ord: i64,
    hash_ok: bool,
    manifest_changed: bool,
    changed: bool,
    removed_ids: Vec<String>,
    rig_candidates: HashSet<String>,
}

impl Run<'_> {
    /// The asset rows an item is made of: (id, rowid, blob).
    fn item_rows(&self, item_id: &str) -> AppResult<Vec<(String, i64, String)>> {
        let mut statement = self.tx.prepare_cached("SELECT id, rowid, asset FROM assets WHERE item_id=?1 OR id=?1")?;
        let rows = statement.query_map([item_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn note_rigs(&mut self, asset_id: &str) -> AppResult<()> {
        let mut statement = self.tx.prepare_cached("SELECT rig_id FROM rig_uses WHERE asset_id=?1")?;
        let found = statement.query_map([asset_id], |row| row.get::<_, String>(0))?;
        for rig in found {
            self.rig_candidates.insert(rig?);
        }
        Ok(())
    }

    /// Delete the asset row `id` and note everything that left with it.
    fn drop_asset(&mut self, id: &str, rowid: i64, blob: &str, gone: &mut Vec<String>) -> AppResult<()> {
        self.note_rigs(id)?;
        let mut ids = vec![id.to_string()];
        if let Ok(value) = serde_json::from_str::<Value>(blob) {
            variant_ids_of(&value, &mut ids);
        }
        catalog::delete_asset_row(self.tx, id, rowid, self.revision, !self.bulk)?;
        for gone_id in &ids {
            self.tx.prepare_cached("DELETE FROM thumbs WHERE asset_id=?1")?.execute([gone_id])?;
        }
        gone.extend(ids);
        self.changed = true;
        Ok(())
    }

    fn remove_item(&mut self, id: &str) -> AppResult<()> {
        let mut gone = Vec::new();
        for (asset_id, rowid, blob) in self.item_rows(id)? {
            self.drop_asset(&asset_id, rowid, &blob, &mut gone)?;
        }
        {
            let mut statement = self.tx.prepare_cached("SELECT variant_id FROM item_variants WHERE item_id=?1")?;
            let found = statement.query_map([id], |row| row.get::<_, String>(0))?;
            for variant in found {
                gone.push(variant?);
            }
        }
        if self.tx.execute("DELETE FROM items WHERE id=?1", [id])? > 0 {
            self.manifest_changed = true;
            self.changed = true;
            gone.push(id.to_string());
        }
        gone.sort();
        gone.dedup();
        catalog::write_users_for(self.user_tx, &BTreeMap::new(), &gone)?;
        self.removed_ids.extend(gone);
        Ok(())
    }

    fn clear_previews(&mut self, ids: &[String]) -> AppResult<()> {
        for chunk in ids.chunks(400) {
            let marks = vec!["?"; chunk.len()].join(",");
            let owners: Vec<String> = {
                let mut statement = self.user_tx.prepare(&format!("SELECT asset_id FROM user_assets WHERE preview_asset IN ({marks})"))?;
                let found = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| row.get(0))?;
                found.collect::<Result<_, _>>()?
            };
            if owners.is_empty() {
                continue;
            }
            self.user_tx.execute(&format!("UPDATE user_assets SET preview_asset=NULL WHERE preview_asset IN ({marks})"), rusqlite::params_from_iter(chunk.iter()))?;
            for owner in owners {
                self.user_tx.execute(
                    "DELETE FROM user_assets WHERE asset_id=?1 AND favorite=0 AND preview_asset IS NULL AND filesystem_path IS NULL AND thumb_error IS NULL AND provenance IS NULL \
                     AND NOT EXISTS (SELECT 1 FROM user_tags WHERE asset_id=?1) AND NOT EXISTS (SELECT 1 FROM user_collections WHERE asset_id=?1)",
                    [owner],
                )?;
            }
        }
        Ok(())
    }

    fn put(&mut self, put: &ItemPut) -> AppResult<()> {
        if let Some(entry) = &put.entry {
            self.put_entry(&put.id, entry)?;
        }
        if self.skip_catalog {
            return Ok(());
        }
        // A bulk download writes into rows that are not there yet: nothing to look for (and the index it would use is not kept).
        let existing = if self.bulk { Vec::new() } else { self.item_rows(&put.id)? };
        let mut prior_ids: Vec<String> = Vec::new();
        for (id, _, blob) in &existing {
            prior_ids.push(id.clone());
            if let Ok(value) = serde_json::from_str::<Value>(blob) {
                variant_ids_of(&value, &mut prior_ids);
            }
        }
        for (id, _, _) in &existing {
            self.note_rigs(id)?;
        }

        let mut users: BTreeMap<String, UserRow> = BTreeMap::new();
        let mut keep: HashSet<&str> = HashSet::new();
        let mut forget: Vec<String> = prior_ids;
        for asset in &put.assets {
            keep.insert(asset.id.as_str());
            let key = crate::ops::content_hash::storage_key(asset);
            let hash = match (&key, self.remote, self.hash_ok) {
                (Some(key), false, true) => self.tx.query_row("SELECT hash FROM hashes WHERE key=?1", [key], |row| row.get::<_, String>(0)).optional()?,
                _ => None,
            };
            let row = catalog::row_in(asset, if self.remote { None } else { Some(put.id.clone()) }, hash, self.remote, &mut users)?;
            forget.push(asset.id.clone());
            forget.extend(row.variant_ids.iter().cloned());
            let old = catalog::old_by_id(self.tx, &asset.id)?;
            let ord = match &old {
                Some(old) => old.ord,
                None => {
                    self.next_ord += 1;
                    self.next_ord - 1
                }
            };
            self.changed |= catalog::write_row(self.tx, &row, old.as_ref(), ord, self.revision, self.hash_ok, !self.bulk)?;
        }
        if put.entry.is_some() {
            let mut gone = Vec::new();
            for (id, rowid, blob) in &existing {
                if !keep.contains(id.as_str()) {
                    self.drop_asset(id, *rowid, blob, &mut gone)?;
                }
            }
            self.removed_ids.extend(gone);
        }
        for rig in &put.rigs {
            // A replica takes the server's rig as it is; a library keeps the one it has.
            catalog::write_rig(self.tx, rig, self.remote)?;
        }
        forget.sort();
        forget.dedup();
        // A bulk download into a replica with no personal data written yet has none to find or replace.
        if !(self.user_empty && users.is_empty()) {
            catalog::write_users_for(self.user_tx, &users, &forget)?;
        }
        Ok(())
    }

    fn put_entry(&mut self, id: &str, entry: &Value) -> AppResult<()> {
        if entry.get("id").and_then(Value::as_str) != Some(id) {
            return Err(AppError::msg(format!("The entry stored for '{id}' has another id")));
        }
        let body = serde_json::to_string(entry)?;
        let columns = ItemColumns::of(entry, id);
        let existing: Option<String> = self.tx.query_row("SELECT entry FROM items WHERE id=?1", [id], |row| row.get(0)).optional()?;
        match existing {
            None => {
                self.tx.execute(
                    "INSERT INTO items(id,seq,kind,type,pack,category,name,storage_path,entry,revision,pkey) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    params![id, self.next_seq, columns.kind, columns.type_id, columns.pack, columns.category, columns.name, columns.storage, body, self.revision, columns.pkey],
                )?;
                self.next_seq += 1;
            }
            Some(previous) if previous == body => return Ok(()),
            Some(_) => {
                self.tx.execute(
                    "UPDATE items SET kind=?2,type=?3,pack=?4,category=?5,name=?6,storage_path=?7,entry=?8,revision=?9,pkey=?10 WHERE id=?1",
                    params![id, columns.kind, columns.type_id, columns.pack, columns.category, columns.name, columns.storage, body, self.revision, columns.pkey],
                )?;
            }
        }
        manifest::sync_variants(self.tx, id, entry)?;
        self.manifest_changed = true;
        self.changed = true;
        Ok(())
    }
}
