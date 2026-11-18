//! The revision feed: what a client needs to catch up with a library without
//! reading the library. Every page comes straight from SQLite through the
//! `changes_revision` index; no catalog is built.
//!
//! * `changes` holds the latest change of each asset id: `(revision, deleted)`.
//!   A put sets `deleted = 0` at the commit's revision, a removal `deleted = 1`.
//! * Live rows are never pruned (they are the current state). Tombstones
//!   (`deleted = 1`) are kept for [`CHANGES_KEEP_REVISIONS`] revisions; pruning
//!   them raises `meta.changesFloor`, and a client whose cursor is older than the
//!   floor is told to `reset` (start again from 0) instead of silently missing a
//!   deletion.
//! * Within a revision rows come puts first, then removals, each by id. A client
//!   that sees an asset absorb the rows of another (a variant group) therefore
//!   holds those rows still when it learns they are gone, and can hand over their
//!   favourites and tags.
//! * A page that ends inside a revision (a commit with more rows than the limit)
//!   carries an opaque cursor `after`; `(next, after)` together say where to go on.
//! * Shared thumbnails are a separate, smaller log (`thumbs`, `meta.thumbRevision`)
//!   that does not move the library revision: a picture is not a library change.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::catalog::{restore, UserRow};
use super::{meta_get, meta_int, meta_set, open_or_create, Kind, Store};
use crate::domain::catalog_model::Asset;
use crate::domain::rig::Rig;
use crate::error::{AppError, AppResult};

/// Tombstones older than this many revisions are dropped; a client that has not
/// synced for longer starts again from 0.
pub const CHANGES_KEEP_REVISIONS: i64 = 20_000;

const CHUNK: usize = 400;

/// The rows after a revision.
pub(crate) const PAGE_FROM_REVISION: &str = "SELECT revision, deleted, asset_id FROM changes WHERE revision > ?1 ORDER BY revision, deleted, asset_id LIMIT ?2";
/// The rows after a cursor inside a revision.
pub(crate) const PAGE_FROM_CURSOR: &str =
    "SELECT revision, deleted, asset_id FROM changes WHERE (revision, deleted, asset_id) > (?1, ?2, ?3) ORDER BY revision, deleted, asset_id LIMIT ?4";

/// Drop tombstones older than `keep` revisions before `revision` (the commit
/// being written) and raise the floor if any went.
pub(crate) fn prune_with(tx: &Connection, revision: i64, keep: i64) -> AppResult<()> {
    let cutoff = revision - keep;
    if cutoff <= 0 {
        return Ok(());
    }
    if tx.prepare_cached("DELETE FROM changes WHERE deleted=1 AND revision<=?1")?.execute([cutoff])? > 0 {
        let floor = meta_int(tx, "changesFloor")?;
        meta_set(tx, "changesFloor", &floor.max(cutoff).to_string())?;
    }
    Ok(())
}

pub(crate) fn prune_in_tx(tx: &Connection, revision: i64) -> AppResult<()> {
    prune_with(tx, revision, CHANGES_KEEP_REVISIONS)
}

/// Which page of the feed to read.
#[derive(Debug, Clone, Copy)]
pub struct ChangesQuery<'a> {
    /// The revision the client has everything up to.
    pub since: i64,
    /// The cursor of the last page when it ended inside a revision.
    pub after: Option<&'a str>,
    /// Most rows (changed plus removed ids) in the page.
    pub limit: usize,
    /// The identity of the database the client's cursor belongs to, if it has one.
    pub db_id: Option<&'a str>,
}

/// One page of the feed: the wire format of `GET /changes`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesPage {
    /// Identity of the library database; another one means the library was replaced.
    #[serde(default)]
    pub db_id: String,
    /// The server cannot answer for the cursor given: forget everything and start from 0.
    #[serde(default)]
    pub reset: bool,
    /// Catalog format of the assets in the page.
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub since: i64,
    /// The revision to ask for next.
    #[serde(default)]
    pub next: i64,
    /// With `next`, where inside that revision to go on (only when `more`).
    #[serde(default)]
    pub after: Option<String>,
    /// The library revision when the page was read.
    #[serde(default)]
    pub revision: i64,
    #[serde(default)]
    pub thumb_revision: i64,
    /// There are rows beyond this page.
    #[serde(default)]
    pub more: bool,
    /// Assets written, as `/catalog` serves them (no personal data).
    #[serde(default)]
    pub assets: Vec<Asset>,
    /// The rigs those assets bind to.
    #[serde(default)]
    pub rigs: Vec<Rig>,
    /// Asset ids that left the catalog.
    #[serde(default)]
    pub deleted: Vec<String>,
}

fn marks(count: usize) -> String {
    vec!["?"; count].join(",")
}

fn collect_variant_hash(connection: &Connection, asset: &mut Asset, ok: bool) -> AppResult<()> {
    for variant in &mut asset.variants {
        variant.content_hash = match (ok, crate::ops::content_hash::storage_key(variant)) {
            (true, Some(key)) => {
                use rusqlite::OptionalExtension;
                connection.prepare_cached("SELECT hash FROM hashes WHERE key=?1")?.query_row([key], |row| row.get::<_, String>(0)).optional()?
            }
            _ => None,
        };
        collect_variant_hash(connection, variant, ok)?;
    }
    Ok(())
}

/// The assets with these ids as a client may see them: library data only, with
/// content hashes (items from their row, variants from the hashes table).
fn served_in(connection: &Connection, ids: &[String]) -> AppResult<Vec<Asset>> {
    let ok = super::hashes::scheme_ok(connection)?;
    let none: HashMap<String, UserRow> = HashMap::new();
    let mut found: HashMap<String, Asset> = HashMap::new();
    for chunk in ids.chunks(CHUNK) {
        let mut statement = connection.prepare(&format!("SELECT id, asset, content_hash FROM assets WHERE id IN ({})", marks(chunk.len())))?;
        let mut rows = statement.query(params_from_iter(chunk.iter()))?;
        while let Some(row) = rows.next()? {
            let text: String = row.get(1)?;
            let mut value: Value = serde_json::from_str(&text)?;
            restore(&mut value, &none)?;
            let mut asset: Asset = serde_json::from_value(value)?;
            asset.content_hash = if ok { row.get(2)? } else { None };
            collect_variant_hash(connection, &mut asset, ok)?;
            found.insert(row.get(0)?, asset);
        }
    }
    Ok(ids.iter().filter_map(|id| found.remove(id)).collect())
}

fn rigs_in(connection: &Connection, assets: &[Asset]) -> AppResult<Vec<Rig>> {
    fn collect(asset: &Asset, out: &mut Vec<String>) {
        out.extend(asset.rigs.iter().map(|binding| binding.rig_id.clone()));
        for variant in &asset.variants {
            collect(variant, out);
        }
    }
    let mut ids = Vec::new();
    for asset in assets {
        collect(asset, &mut ids);
    }
    ids.sort();
    ids.dedup();
    let mut found: Vec<(i64, Rig)> = Vec::new();
    for chunk in ids.chunks(CHUNK) {
        let mut statement = connection.prepare(&format!("SELECT rowid, rig FROM rigs WHERE id IN ({})", marks(chunk.len())))?;
        let mut rows = statement.query(params_from_iter(chunk.iter()))?;
        while let Some(row) = rows.next()? {
            let text: String = row.get(1)?;
            found.push((row.get(0)?, serde_json::from_str(&text)?));
        }
    }
    found.sort_by_key(|(rowid, _)| *rowid);
    Ok(found.into_iter().map(|(_, rig)| rig).collect())
}

fn parse_cursor(after: &str) -> AppResult<(i64, String)> {
    match after.split_once(':') {
        Some(("0", id)) if !id.is_empty() => Ok((0, id.to_string())),
        Some(("1", id)) if !id.is_empty() => Ok((1, id.to_string())),
        _ => Err(AppError::msg("The cursor is not one this server gave out")),
    }
}

fn page_in(connection: &Connection, query: &ChangesQuery) -> AppResult<ChangesPage> {
    let revision = meta_int(connection, "revision")?;
    let mut page = ChangesPage {
        db_id: meta_get(connection, "dbId")?.unwrap_or_default(),
        schema_version: crate::config::SCHEMA_VERSION,
        since: query.since,
        next: revision,
        revision,
        thumb_revision: meta_int(connection, "thumbRevision")?,
        ..Default::default()
    };
    let floor = meta_int(connection, "changesFloor")?;
    let foreign = query.db_id.is_some_and(|known| !known.is_empty() && known != page.db_id);
    if foreign || query.since > revision || (query.since > 0 && query.since < floor) {
        page.reset = true;
        page.next = 0;
        return Ok(page);
    }
    let limit = query.limit.max(1);
    let mut rows: Vec<(i64, i64, String)> = Vec::new();
    {
        let read = |row: &rusqlite::Row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?));
        match query.after {
            Some(after) => {
                let (deleted, id) = parse_cursor(after)?;
                let mut statement = connection.prepare_cached(PAGE_FROM_CURSOR)?;
                let found = statement.query_map(params![query.since, deleted, id, limit as i64 + 1], read)?;
                for row in found {
                    rows.push(row?);
                }
            }
            None => {
                let mut statement = connection.prepare_cached(PAGE_FROM_REVISION)?;
                let found = statement.query_map(params![query.since, limit as i64 + 1], read)?;
                for row in found {
                    rows.push(row?);
                }
            }
        }
    }
    if rows.len() > limit {
        rows.truncate(limit);
        page.more = true;
        let (last_revision, last_deleted, last_id) = &rows[limit - 1];
        page.next = *last_revision;
        page.after = Some(format!("{last_deleted}:{last_id}"));
    }
    let mut put_ids = Vec::new();
    for (_, deleted, id) in rows {
        if deleted == 0 {
            put_ids.push(id);
        } else {
            page.deleted.push(id);
        }
    }
    page.assets = served_in(connection, &put_ids)?;
    page.rigs = rigs_in(connection, &page.assets)?;
    Ok(page)
}

impl Store {
    /// A page of the revision feed (see the module comment). Without a database
    /// the page is empty, at revision 0.
    pub fn changes_page(&self, query: &ChangesQuery) -> AppResult<ChangesPage> {
        match self.read(|library| page_in(library, query))? {
            Some(page) => Ok(page),
            None => Ok(ChangesPage { schema_version: crate::config::SCHEMA_VERSION, since: query.since, ..Default::default() }),
        }
    }

    /// The top-level assets with these ids as a client may see them, in the order
    /// asked; ids that are not assets are left out.
    pub fn served_assets(&self, ids: &[String]) -> AppResult<Vec<Asset>> {
        Ok(self.read(|library| served_in(library, ids))?.unwrap_or_default())
    }

    /// Every id a client may ask a picture for: assets and variants, sorted.
    pub fn all_ids(&self) -> AppResult<Vec<String>> {
        Ok(self
            .read(|library| {
                let mut statement = library.prepare("SELECT id FROM assets UNION SELECT variant_id FROM item_variants ORDER BY 1")?;
                let found = statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// The ids (assets and variants) whose picture would be the file named by one of `names` (sanitised ids; see
    /// `LibraryHandle::thumb_path`). One lookup per name through `assets_thumb` / `item_variants_thumb`.
    pub fn ids_by_thumb_name(&self, names: &[String]) -> AppResult<Vec<String>> {
        Ok(self
            .read(|library| {
                let mut found: HashSet<String> = HashSet::new();
                for chunk in names.chunks(CHUNK) {
                    for sql in ["SELECT id FROM assets WHERE thumb_name IN", "SELECT variant_id FROM item_variants WHERE thumb_name IN"] {
                        let mut statement = library.prepare(&format!("{sql} ({})", marks(chunk.len())))?;
                        let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                        while let Some(row) = rows.next()? {
                            found.insert(row.get::<_, String>(0)?);
                        }
                    }
                }
                let mut ids: Vec<String> = found.into_iter().collect();
                ids.sort();
                Ok(ids)
            })?
            .unwrap_or_default())
    }

    /// Which of `ids` name an asset or a variant of the library.
    pub fn known_ids(&self, ids: &[String]) -> AppResult<HashSet<String>> {
        Ok(self
            .read(|library| {
                let mut found = HashSet::new();
                for chunk in ids.chunks(CHUNK) {
                    for sql in ["SELECT id FROM assets WHERE id IN", "SELECT variant_id FROM item_variants WHERE variant_id IN"] {
                        let mut statement = library.prepare(&format!("{sql} ({})", marks(chunk.len())))?;
                        let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                        while let Some(row) = rows.next()? {
                            found.insert(row.get::<_, String>(0)?);
                        }
                    }
                }
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// The top-level assets that are `ids` or contain one of them as a variant.
    pub fn owner_asset_ids(&self, ids: &[String]) -> AppResult<Vec<String>> {
        Ok(self
            .read(|library| {
                let mut owners: Vec<String> = Vec::new();
                let mut seen = HashSet::new();
                let mut items: Vec<String> = Vec::new();
                for chunk in ids.chunks(CHUNK) {
                    let mut statement = library.prepare(&format!("SELECT id FROM assets WHERE id IN ({})", marks(chunk.len())))?;
                    let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                    while let Some(row) = rows.next()? {
                        let id: String = row.get(0)?;
                        if seen.insert(id.clone()) {
                            owners.push(id);
                        }
                    }
                    let mut statement = library.prepare(&format!("SELECT item_id FROM item_variants WHERE variant_id IN ({})", marks(chunk.len())))?;
                    let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                    while let Some(row) = rows.next()? {
                        items.push(row.get(0)?);
                    }
                }
                items.sort();
                items.dedup();
                let mut statement = library.prepare_cached("SELECT id FROM assets WHERE item_id=?1 OR id=?1")?;
                for item in &items {
                    let found = statement.query_map([item], |row| row.get::<_, String>(0))?;
                    for id in found {
                        let id = id?;
                        if seen.insert(id.clone()) {
                            owners.push(id);
                        }
                    }
                }
                Ok(owners)
            })?
            .unwrap_or_default())
    }

    /// Drop tombstones older than `keep` revisions (a commit does this with
    /// [`CHANGES_KEEP_REVISIONS`]). A client that last synced before the floor is reset.
    pub fn prune_changes(&self, keep: i64) -> AppResult<()> {
        let mut connection = open_or_create(&self.library, Kind::Library)?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        prune_with(&tx, meta_int(&tx, "revision")?, keep)?;
        tx.commit()?;
        Ok(())
    }

    // ---- shared thumbnails ----------------------------------------------------------

    /// Record that the pictures of these assets (or variants) were saved; returns
    /// the new thumbnail revision. The library revision does not move.
    pub fn note_thumbs(&self, ids: &[String]) -> AppResult<i64> {
        let mut connection = open_or_create(&self.library, Kind::Library)?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let revision = meta_int(&tx, "thumbRevision")? + 1;
        for id in ids {
            tx.prepare_cached("INSERT INTO thumbs(asset_id,revision) VALUES(?1,?2) ON CONFLICT(asset_id) DO UPDATE SET revision=excluded.revision")?.execute(params![id, revision])?;
        }
        meta_set(&tx, "thumbRevision", &revision.to_string())?;
        tx.commit()?;
        Ok(revision)
    }

    /// The ids whose picture was saved after thumbnail revision `since`, and the
    /// current thumbnail revision (read together).
    pub fn thumbs_since(&self, since: i64) -> AppResult<(Vec<String>, i64)> {
        Ok(self
            .read(|library| {
                let revision = meta_int(library, "thumbRevision")?;
                let mut statement = library.prepare("SELECT asset_id FROM thumbs WHERE revision > ?1 ORDER BY revision, asset_id")?;
                let ids = statement.query_map([since], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
                Ok((ids, revision))
            })?
            .unwrap_or_default())
    }

    // ---- replicas -------------------------------------------------------------------

    /// Empty a replica's library rows and its sync cursor, keeping `user.db`
    /// (favourites, personal tags and collections wait there for their assets to
    /// come back). Only valid for a remote library.
    pub fn clear_replica(&self) -> AppResult<()> {
        if !self.remote {
            return Err(AppError::msg("Only the replica of a remote library can be emptied"));
        }
        let mut connection = open_or_create(&self.library, Kind::Library)?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for table in ["asset_search", "assets", "rigs", "changes", "thumbs"] {
            tx.execute(&format!("DELETE FROM {table}"), [])?;
        }
        for key in ["catalogSchema", "remoteRevision", "remoteAfter", "remoteDbId", "remoteThumbRevision"] {
            super::meta_delete(&tx, key)?;
        }
        meta_set(&tx, "revision", &(meta_int(&tx, "revision")? + 1).to_string())?;
        tx.commit()?;
        Ok(())
    }

    /// Whether the replica is in the middle of a bulk download (see [`Store::begin_bulk`]).
    pub fn is_bulk(&self) -> bool {
        super::open_existing(&self.library, Kind::Library).ok().flatten().and_then(|c| meta_get(&c, "bulkLoad").ok().flatten()).as_deref() == Some("1")
    }

    /// Start a bulk download into the replica: its rows are emptied (`user.db` is kept) and what only the finished
    /// replica needs (most indexes, the counts triggers, the search table) is dropped, so that rows are written
    /// without keeping those up. The replica is marked (`bulkLoad`) and is not read until [`Store::finish_bulk`]
    /// makes them again, in one pass each. An interrupted download goes on from its cursor and finishes later.
    pub fn begin_bulk(&self) -> AppResult<()> {
        if !self.remote {
            return Err(AppError::msg("Only the replica of a remote library can be loaded in bulk"));
        }
        let mut connection = open_or_create(&self.library, Kind::Library)?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for deferred in super::deferred_schema() {
            let what = match deferred.kind {
                "index" => "INDEX",
                "trigger" => "TRIGGER",
                _ => "TABLE",
            };
            tx.execute_batch(&format!("DROP {what} IF EXISTS \"{}\"", deferred.name))?;
        }
        for table in ["asset_tags", "asset_files", "rig_uses", "asset_hash_keys", "changes", "thumbs", "facets", "rigs", "assets"] {
            tx.execute(&format!("DELETE FROM {table}"), [])?;
        }
        for key in ["catalogSchema", "remoteRevision", "remoteAfter", "remoteDbId", "remoteThumbRevision"] {
            super::meta_delete(&tx, key)?;
        }
        meta_set(&tx, "bulkLoad", "1")?;
        meta_set(&tx, "revision", &(meta_int(&tx, "revision")? + 1).to_string())?;
        tx.commit()?;
        Ok(())
    }

    /// The end of a bulk download: the indexes, counts and search rows are made from the rows that are there, the
    /// triggers are put back, and the replica is read again. One transaction: a crash in it leaves the replica in bulk mode.
    pub fn finish_bulk(&self) -> AppResult<()> {
        let mut connection = open_or_create(&self.library, Kind::Library)?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if meta_get(&tx, "bulkLoad")?.as_deref() != Some("1") {
            return Ok(());
        }
        let trace = std::env::var("TOYBOX_TRACE_SYNC").is_ok();
        let began = std::time::Instant::now();
        // Sorting a million keys for an index: in memory, not in many small runs on disk.
        tx.execute_batch("PRAGMA cache_size=-1048576; PRAGMA temp_store=MEMORY;")?;
        let deferred = super::deferred_schema();
        for item in deferred.iter().filter(|d| d.kind == "index") {
            tx.execute_batch(&item.sql)?;
            if trace {
                eprintln!("    {} {:.0} ms", item.name, began.elapsed().as_secs_f64() * 1000.0);
            }
        }
        tx.execute_batch(
            "INSERT INTO facets(type,field,value,n) SELECT type,'type','',COUNT(*) FROM assets GROUP BY type;
             INSERT INTO facets(type,field,value,n) SELECT type,'category',category,COUNT(*) FROM assets GROUP BY type,category;
             INSERT INTO facets(type,field,value,n) SELECT type,'source',source,COUNT(*) FROM assets GROUP BY type,source;
             INSERT INTO facets(type,field,value,n) SELECT type,'publisher',publisher,COUNT(*) FROM assets GROUP BY type,publisher;
             INSERT INTO facets(type,field,value,n) SELECT type,'license',license,COUNT(*) FROM assets GROUP BY type,license;
             INSERT INTO facets(type,field,value,n) SELECT type,'format',format,COUNT(*) FROM assets GROUP BY type,format;
             INSERT INTO facets(type,field,value,n) SELECT type,'tag',tag,COUNT(*) FROM asset_tags GROUP BY type,tag;",
        )?;
        if trace {
            eprintln!("    facets {:.0} ms", began.elapsed().as_secs_f64() * 1000.0);
        }
        for item in deferred.iter().filter(|d| d.kind == "fts") {
            tx.execute_batch(&item.sql)?;
        }
        tx.execute_batch(
            "INSERT INTO asset_search(rowid,name,tags) SELECT rowid, name, COALESCE((SELECT group_concat(value, ' ') FROM json_each(assets.asset, '$.tags')), '') FROM assets",
        )?;
        if trace {
            eprintln!("    fts {:.0} ms", began.elapsed().as_secs_f64() * 1000.0);
        }
        for item in deferred.iter().filter(|d| d.kind == "trigger") {
            tx.execute_batch(&item.sql)?;
        }
        super::meta_delete(&tx, "bulkLoad")?;
        meta_set(&tx, "revision", &(meta_int(&tx, "revision")? + 1).to_string())?;
        tx.commit()?;
        Ok(())
    }

    /// Put the personal data kept for these assets (and their variants) on them: a
    /// favourite, personal tags, collections, a preview choice, the importer's
    /// path and a thumbnail error, read by id from `user.db`. Used on assets a
    /// server sent, which carry none.
    pub fn overlay_personal(&self, assets: Vec<Asset>) -> AppResult<Vec<Asset>> {
        fn collect(asset: &Asset, out: &mut Vec<String>) {
            out.push(asset.id.clone());
            for variant in &asset.variants {
                collect(variant, out);
            }
        }
        fn blob(asset: &Asset) -> AppResult<Value> {
            // The form `assets.asset` keeps: no `user`, no `thumb`, the public provenance beside.
            let mut value = serde_json::to_value(asset)?;
            let object = value.as_object_mut().ok_or_else(|| AppError::msg("An asset is not an object"))?;
            object.remove("user");
            object.remove("thumb");
            if let Some(provenance) = &asset.user.provenance {
                let mut public = provenance.clone();
                public.filesystem_path = None;
                object.insert("provenance".into(), serde_json::to_value(public)?);
            }
            if let Some(variants) = object.get_mut("variants").and_then(Value::as_array_mut) {
                for (variant, typed) in variants.iter_mut().zip(&asset.variants) {
                    *variant = blob(typed)?;
                }
            }
            Ok(value)
        }
        let mut ids = Vec::new();
        for asset in &assets {
            collect(asset, &mut ids);
        }
        let users = self.user_rows(&ids)?;
        if users.is_empty() {
            return Ok(assets);
        }
        let mut out = Vec::with_capacity(assets.len());
        for asset in assets {
            let mut value = blob(&asset)?;
            restore(&mut value, &users)?;
            out.push(serde_json::from_value::<Asset>(value)?);
        }
        Ok(out)
    }
}
