//! The manifest as rows of `items`. `read_manifest` rebuilds the document the
//! JSON file used to be; `write_manifest` diffs a document against the rows and
//! writes only what changed. Entries are stored verbatim, so unknown keys and
//! key order inside an entry survive; unknown top-level keys live in
//! `meta.extra` and their order in `meta.topOrder`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde_json::{Map, Value};

use super::{library_db, meta_delete, meta_get, meta_int, meta_set, open_existing, open_or_create, Kind};
use crate::error::{AppError, AppResult};

/// Top-level keys with a home of their own in `meta`.
const SCHEMA_KEY: (&str, &str) = ("schemaVersion", "manifestSchemaVersion");
const STORAGE_KEY: (&str, &str) = ("storageVersion", "storageVersion");

/// Whether the library at `root` has a manifest in its database.
pub fn manifest_exists(root: &Path) -> bool {
    open_existing(&library_db(root), Kind::Library).ok().flatten().is_some_and(|c| has_manifest(&c))
}

pub(crate) fn has_manifest(connection: &Connection) -> bool {
    meta_get(connection, "hasManifest").ok().flatten().is_some()
}

/// The manifest document, as it was written.
pub fn read_manifest(root: &Path) -> AppResult<Value> {
    read_manifest_with_revision(root).map(|(doc, _)| doc)
}

/// The manifest and the manifest revision it is at, read together.
pub fn read_manifest_with_revision(root: &Path) -> AppResult<(Value, i64)> {
    let path = library_db(root);
    let Some(connection) = open_existing(&path, Kind::Library)? else {
        return Err(AppError::msg(format!("not a Toybox library: no library database in {}", root.display())));
    };
    connection.execute_batch("BEGIN")?;
    let result = (|| {
        if !has_manifest(&connection) {
            return Err(AppError::msg(format!("not a Toybox library: no manifest in {}", root.display())));
        }
        Ok((read_conn(&connection)?, meta_int(&connection, "manifestRevision")?))
    })();
    let _ = connection.execute_batch("COMMIT");
    result
}

/// The revision of the manifest (0 when there is none). Changes whenever a
/// write changes the manifest; what callers that used to stat the file compare.
pub fn manifest_revision(root: &Path) -> AppResult<i64> {
    let Some(connection) = open_existing(&library_db(root), Kind::Library)? else { return Ok(0) };
    meta_int(&connection, "manifestRevision")
}

/// Make the manifest rows equal `doc`. One transaction; the revision goes up by
/// one if anything changed, and the changed rows carry it.
pub fn write_manifest(root: &Path, doc: &Value) -> AppResult<()> {
    let mut connection = open_or_create(&library_db(root), Kind::Library)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let revision = meta_int(&tx, "revision")? + 1;
    if write_conn(&tx, doc, revision)? {
        meta_set(&tx, "revision", &revision.to_string())?;
        meta_set(&tx, "manifestRevision", &revision.to_string())?;
    }
    tx.commit()?;
    Ok(())
}

pub(crate) fn read_conn(connection: &Connection) -> AppResult<Value> {
    let mut entries: Vec<Value> = Vec::new();
    {
        let mut statement = connection.prepare("SELECT entry FROM items ORDER BY seq")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let text: String = row.get(0)?;
            entries.push(serde_json::from_str(&text)?);
        }
    }
    let extra: Map<String, Value> = match meta_get(connection, "extra")? {
        Some(text) => serde_json::from_str(&text)?,
        None => Map::new(),
    };
    let order: Vec<String> = match meta_get(connection, "topOrder")? {
        Some(text) => serde_json::from_str(&text)?,
        None => Vec::new(),
    };
    let known = |meta_key: &str| -> AppResult<Option<Value>> { meta_get(connection, meta_key)?.map(|text| Ok(serde_json::from_str(&text)?)).transpose() };
    let mut schema = known(SCHEMA_KEY.1)?;
    let mut storage = known(STORAGE_KEY.1)?;
    let mut extra = extra;
    let mut assets = Some(Value::Array(entries));
    let mut doc = Map::new();
    for key in &order {
        let value = match key.as_str() {
            "assets" => assets.take(),
            k if k == SCHEMA_KEY.0 => schema.take(),
            k if k == STORAGE_KEY.0 => storage.take(),
            k => extra.shift_remove(k),
        };
        if let Some(value) = value {
            doc.insert(key.clone(), value);
        }
    }
    // Keys the recorded order does not name (a database written without it).
    if let Some(value) = schema { doc.insert(SCHEMA_KEY.0.into(), value); }
    if let Some(value) = storage { doc.insert(STORAGE_KEY.0.into(), value); }
    for (key, value) in extra { doc.insert(key, value); }
    if let Some(value) = assets { doc.insert("assets".into(), value); }
    Ok(Value::Object(doc))
}

pub(crate) fn text<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry.get(key).and_then(Value::as_str).unwrap_or("")
}

struct Existing {
    seq: i64,
    entry: String,
}

/// The columns of an `items` row derived from its entry.
pub(crate) struct ItemColumns<'a> {
    pub kind: &'a str,
    pub type_id: &'a str,
    pub pack: &'a str,
    pub category: &'a str,
    pub name: &'a str,
    pub storage: Option<&'a str>,
    pub pkey: Option<String>,
}

impl<'a> ItemColumns<'a> {
    pub(crate) fn of(entry: &'a Value, id: &'a str) -> ItemColumns<'a> {
        let kind = entry.get("kind").and_then(Value::as_str).unwrap_or("model");
        ItemColumns {
            kind,
            type_id: if kind == "plugin" { text(entry, "type") } else { kind },
            pack: text(entry, "pack"),
            category: text(entry, "category"),
            name: entry.get("name").and_then(Value::as_str).or_else(|| entry.get("file").and_then(Value::as_str)).unwrap_or(id),
            storage: entry.get("storagePath").and_then(Value::as_str),
            pkey: crate::domain::seed::physical_key_text(entry),
        }
    }
}

/// Make `item_variants` list exactly the variants of `entry`.
pub(crate) fn sync_variants(tx: &Connection, id: &str, entry: &Value) -> AppResult<()> {
    tx.execute("DELETE FROM item_variants WHERE item_id=?1", [id])?;
    let mut insert = tx.prepare_cached("INSERT OR IGNORE INTO item_variants(variant_id,item_id,thumb_name) VALUES(?1,?2,?3)")?;
    for variant in entry.get("variants").and_then(Value::as_array).into_iter().flatten() {
        if let Some(variant_id) = variant.get("id").and_then(Value::as_str) {
            insert.execute(params![variant_id, id, crate::infra::appdata::sanitize_id(variant_id)])?;
        }
    }
    Ok(())
}

/// Write `doc` into the open transaction; true if anything changed. `revision`
/// is the revision the changed rows are stamped with. Does not bump `meta`.
pub(crate) fn write_conn(tx: &Transaction, doc: &Value, revision: i64) -> AppResult<bool> {
    let object = doc.as_object().ok_or_else(|| AppError::msg("The library manifest is not a JSON object"))?;
    let assets = object.get("assets").and_then(Value::as_array).ok_or_else(|| AppError::msg("Library manifest has no assets"))?;
    let mut changed = false;

    // Top-level keys.
    let order: Vec<&str> = object.keys().map(String::as_str).collect();
    let mut extra = Map::new();
    for (key, value) in object {
        if key != "assets" && key != SCHEMA_KEY.0 && key != STORAGE_KEY.0 {
            extra.insert(key.clone(), value.clone());
        }
    }
    changed |= meta_set(tx, "topOrder", &serde_json::to_string(&order)?)?;
    changed |= meta_set(tx, "extra", &serde_json::to_string(&extra)?)?;
    for (top, meta) in [SCHEMA_KEY, STORAGE_KEY] {
        changed |= match object.get(top) {
            Some(value) => meta_set(tx, meta, &serde_json::to_string(value)?)?,
            None => meta_delete(tx, meta)?,
        };
    }
    changed |= meta_set(tx, "hasManifest", "1")?;

    // Items.
    let mut existing: HashMap<String, Existing> = HashMap::new();
    {
        let mut statement = tx.prepare("SELECT id, seq, entry FROM items")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            existing.insert(row.get(0)?, Existing { seq: row.get(1)?, entry: row.get(2)? });
        }
    }
    let mut ids: Vec<&str> = Vec::with_capacity(assets.len());
    let mut seen = HashSet::new();
    for entry in assets {
        let id = entry.get("id").and_then(Value::as_str).ok_or_else(|| AppError::msg("A manifest entry has no id"))?;
        if !seen.insert(id) {
            return Err(AppError::msg(format!("The manifest lists '{id}' twice")));
        }
        ids.push(id);
    }
    // Order: existing items keep their `seq` (a deletion leaves a gap, a new item
    // goes to the end); only a reorder renumbers.
    let mut old_order: Vec<(&String, i64)> = existing.iter().map(|(id, row)| (id, row.seq)).collect();
    old_order.sort_by_key(|(_, seq)| *seq);
    let wanted: HashSet<&str> = ids.iter().copied().collect();
    let kept_old: Vec<&str> = old_order.iter().map(|(id, _)| id.as_str()).filter(|id| wanted.contains(id)).collect();
    let kept_new: Vec<&str> = ids.iter().copied().filter(|id| existing.contains_key(*id)).collect();
    let first_new = ids.iter().position(|id| !existing.contains_key(*id)).unwrap_or(ids.len());
    let appended_only = ids[first_new..].iter().all(|id| !existing.contains_key(*id));
    let incremental = kept_old == kept_new && appended_only;
    let mut next_seq = old_order.last().map(|(_, seq)| *seq + 1).unwrap_or(0);
    let mut seqs: HashMap<&str, i64> = HashMap::new();
    if incremental {
        for id in &kept_new {
            seqs.insert(id, existing[*id].seq);
        }
        for id in &ids[first_new..] {
            seqs.insert(id, next_seq);
            next_seq += 1;
        }
    } else {
        for (index, id) in ids.iter().enumerate() {
            seqs.insert(id, index as i64);
        }
        // Park the rows under unique negative numbers so renumbering cannot collide.
        tx.execute("UPDATE items SET seq = -rowid - 1", [])?;
        changed = true;
    }

    for (id, row) in &existing {
        if !wanted.contains(id.as_str()) {
            tx.execute("DELETE FROM items WHERE id=?1", [id])?;
            let _ = row;
            changed = true;
        }
    }
    let mut insert = tx.prepare_cached("INSERT INTO items(id,seq,kind,type,pack,category,name,storage_path,entry,revision,pkey) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)")?;
    let mut update = tx.prepare_cached("UPDATE items SET seq=?2,kind=?3,type=?4,pack=?5,category=?6,name=?7,storage_path=?8,entry=?9,revision=?10,pkey=?11 WHERE id=?1")?;
    let mut restore = tx.prepare_cached("UPDATE items SET seq=?2 WHERE id=?1")?;
    for (entry, id) in assets.iter().zip(&ids) {
        let body = serde_json::to_string(entry)?;
        let seq = seqs[id];
        let columns = ItemColumns::of(entry, id);
        match existing.get(*id) {
            None => {
                insert.execute(params![id, seq, columns.kind, columns.type_id, columns.pack, columns.category, columns.name, columns.storage, body, revision, columns.pkey])?;
                sync_variants(tx, id, entry)?;
                changed = true;
            }
            Some(row) if row.entry != body || row.seq != seq => {
                update.execute(params![id, seq, columns.kind, columns.type_id, columns.pack, columns.category, columns.name, columns.storage, body, revision, columns.pkey])?;
                if row.entry != body {
                    sync_variants(tx, id, entry)?;
                }
                changed = true;
            }
            Some(_) => {
                // Unchanged, but parked under a negative `seq` while renumbering.
                if !incremental {
                    restore.execute(params![id, seq])?;
                }
            }
        }
    }
    Ok(changed)
}

/// The manifest of the database file at `path` (used to check a database that is not installed yet).
pub(crate) fn read_at(path: &Path) -> AppResult<Value> {
    let connection = open_existing(path, Kind::Library)?.ok_or_else(|| AppError::msg("The library database is missing"))?;
    read_conn(&connection)
}

/// [`write_manifest`], refused unless the manifest is still at `expected` (the
/// revision [`read_manifest_with_revision`] returned): what used to be "the
/// manifest file is unchanged since I read it".
pub fn write_manifest_checked(root: &Path, doc: &Value, expected: i64) -> AppResult<()> {
    let store = super::Store { library: library_db(root), user: library_db(root).with_file_name("user.db"), root: root.to_string_lossy().to_string(), remote: false };
    store.commit(super::CatalogWrite { manifest: Some(doc), catalog: None, expect_manifest_revision: Some(expected) }).map(|_| ())
}
