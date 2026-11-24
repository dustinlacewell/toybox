//! The catalog (derived rows plus personal data) as rows.
//!
//! An `Asset` is split in two. The library part goes to `library.db`: the asset
//! as JSON in `assets.asset` without `user`, `thumb` and (for a local library)
//! `contentHash`, plus its public provenance under a `provenance` key, with the
//! columns, tags and search row derived from it. Variants stay nested in their
//! parent's JSON. The personal part (`favorite`, personal tags, collections, the
//! preview choice, the importer's folder, a thumbnail error) goes to `user.db`,
//! keyed by the id of the asset or variant it belongs to.
//!
//! A save is a diff: rows that are equal are not touched.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde_json::Value;

use super::{hashes, manifest, meta_get, meta_int, meta_set, open_existing, open_or_create, Kind, Store};
use crate::domain::catalog_model::{Asset, AssetContent, Catalog, ThumbMeta, ThumbState, UserMeta};
use crate::domain::provenance::ImportProvenance;
use crate::error::{AppError, AppResult};

/// One atomic change of a library's databases. Any part may be absent.
#[derive(Default, Clone, Copy)]
pub struct CatalogWrite<'a> {
    pub manifest: Option<&'a Value>,
    pub catalog: Option<&'a Catalog>,
    /// Refuse (and write nothing) unless the manifest is still at this revision.
    pub expect_manifest_revision: Option<i64>,
}

/// What a commit did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Committed {
    /// The library revision after the commit.
    pub revision: i64,
    /// Whether anything but bookkeeping changed (the revision went up).
    pub changed: bool,
}

impl Store {
    /// Write the manifest and/or the catalog in one transaction on `library.db`
    /// (plus the `user.db` transaction opened before it and committed after it).
    /// The revision goes up by one if anything changed.
    pub fn commit(&self, write: CatalogWrite) -> AppResult<Committed> {
        if write.manifest.is_some() && self.remote {
            return Err(AppError::msg("A remote library has no manifest here"));
        }
        let mut library = open_or_create(&self.library, Kind::Library)?;
        let mut user = match write.catalog {
            Some(_) => Some(open_or_create(&self.user, Kind::User)?),
            None => None,
        };
        // The personal database is locked first: if it cannot be written, nothing is.
        let user_tx = user.as_mut().map(|connection| connection.transaction_with_behavior(TransactionBehavior::Immediate)).transpose()?;
        let tx = library.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(expected) = write.expect_manifest_revision {
            if meta_int(&tx, "manifestRevision")? != expected {
                return Err(AppError::msg("The library changed while it was being edited. Try again."));
            }
        }
        let previous = meta_int(&tx, "revision")?;
        let revision = previous + 1;
        let mut changed = false;
        if let Some(doc) = write.manifest {
            if manifest::write_conn(&tx, doc, revision)? {
                meta_set(&tx, "manifestRevision", &revision.to_string())?;
                changed = true;
            }
        }
        if let (Some(catalog), Some(user_tx)) = (write.catalog, user_tx.as_ref()) {
            changed |= write_catalog(&tx, user_tx, catalog, self.remote, revision)?;
            meta_set(&tx, "catalogSchema", &catalog.schema_version.to_string())?;
            meta_set(&tx, "derivedFor", &meta_int(&tx, "manifestRevision")?.to_string())?;
        }
        if changed {
            meta_set(&tx, "revision", &revision.to_string())?;
            super::changes::prune_in_tx(&tx, revision)?;
        }
        tx.commit()?;
        if let Some(user_tx) = user_tx {
            user_tx.commit()?;
        }
        Ok(Committed { revision: if changed { revision } else { previous }, changed })
    }

    /// Make the saved catalog equal `catalog`.
    pub fn save_catalog(&self, catalog: &Catalog) -> AppResult<Committed> {
        self.commit(CatalogWrite { catalog: Some(catalog), ..Default::default() })
    }

    /// The saved catalog, or `None` if none was saved. Thumbnail state is only
    /// kept for errors; every other state reads as `missing` (the files on disk
    /// are what says a thumbnail is ready).
    pub fn load_catalog(&self) -> AppResult<Option<Catalog>> {
        let Some(library) = open_existing(&self.library, Kind::Library)? else { return Ok(None) };
        library.execute_batch("BEGIN")?;
        let result = self.load_inner(&library);
        let _ = library.execute_batch("COMMIT");
        result
    }

    fn load_inner(&self, library: &Connection) -> AppResult<Option<Catalog>> {
        let Some(schema) = meta_get(library, "catalogSchema")? else { return Ok(None) };
        let users = load_users(&self.user)?;
        let mut rigs = Vec::new();
        {
            let mut statement = library.prepare("SELECT rig FROM rigs ORDER BY rowid")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let text: String = row.get(0)?;
                rigs.push(serde_json::from_str(&text)?);
            }
        }
        let mut assets = Vec::new();
        {
            let mut statement = library.prepare("SELECT asset FROM assets ORDER BY ord")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let text: String = row.get(0)?;
                let mut value: Value = serde_json::from_str(&text)?;
                restore(&mut value, &users)?;
                assets.push(serde_json::from_value::<Asset>(value)?);
            }
        }
        Ok(Some(Catalog { rigs, schema_version: schema.parse().unwrap_or(0), library_root: self.root.clone(), assets }))
    }

    /// Forget the saved catalog: the derived rows, the rigs, the thumbnail errors
    /// and the personal data kept for them (what deleting the old cache file did).
    /// The manifest and the content hashes stay. A scan builds the catalog again.
    pub fn discard_catalog(&self) -> AppResult<()> {
        if let Some(mut user) = open_existing(&self.user, Kind::User)? {
            let tx = user.transaction_with_behavior(TransactionBehavior::Immediate)?;
            for table in ["user_assets", "user_tags", "user_collections"] {
                tx.execute(&format!("DELETE FROM {table}"), [])?;
            }
            tx.commit()?;
        }
        if let Some(mut library) = open_existing(&self.library, Kind::Library)? {
            let tx = library.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let revision = meta_int(&tx, "revision")? + 1;
            let gone: Vec<String> = {
                let mut statement = tx.prepare("SELECT id FROM assets")?;
                let found = statement.query_map([], |row| row.get(0))?;
                found.collect::<Result<_, _>>()?
            };
            for id in &gone {
                tx.execute("INSERT OR REPLACE INTO changes(revision,asset_id,deleted) VALUES(?1,?2,1)", params![revision, id])?;
            }
            tx.execute("DELETE FROM asset_search", [])?;
            tx.execute("DELETE FROM assets", [])?;
            tx.execute("DELETE FROM rigs", [])?;
            super::meta_delete(&tx, "catalogSchema")?;
            super::meta_delete(&tx, "derivedFor")?;
            if !gone.is_empty() {
                meta_set(&tx, "revision", &revision.to_string())?;
            }
            tx.commit()?;
        }
        Ok(())
    }

    /// The saved assets as plain JSON (what the cache file's `assets` was), or
    /// empty when there is none.
    pub fn catalog_assets_json(&self) -> Vec<Value> {
        match self.load_catalog() {
            Ok(Some(catalog)) => catalog.assets.iter().filter_map(|asset| serde_json::to_value(asset).ok()).collect(),
            _ => Vec::new(),
        }
    }
}

// ---- personal data -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct UserRow {
    pub(crate) favorite: bool,
    pub(crate) preview: Option<String>,
    pub(crate) filesystem_path: Option<String>,
    /// The `ThumbMeta` as JSON, only for an error.
    pub(crate) thumb: Option<String>,
    /// `{"verbatimTags":true}` when `tags` are not "derived from provenance, then the rest".
    pub(crate) flags: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) collections: Vec<String>,
}

pub(crate) fn user_row(asset: &Asset) -> AppResult<Option<UserRow>> {
    let derived = asset.user.provenance.as_ref().map(ImportProvenance::tags).unwrap_or_default();
    let verbatim = !asset.user.tags.starts_with(&derived);
    let row = UserRow {
        favorite: asset.user.favorite,
        preview: asset.user.preview_asset_id.clone(),
        filesystem_path: asset.user.provenance.as_ref().and_then(|p| p.filesystem_path.clone()),
        thumb: if matches!(asset.thumb.state, ThumbState::Error) || asset.thumb.error.is_some() { Some(serde_json::to_string(&asset.thumb)?) } else { None },
        flags: verbatim.then(|| "{\"verbatimTags\":true}".to_string()),
        tags: if verbatim { asset.user.tags.clone() } else { asset.user.tags[derived.len()..].to_vec() },
        collections: asset.user.collections.clone(),
    };
    Ok(if row == UserRow::default() { None } else { Some(row) })
}

fn load_users(path: &std::path::Path) -> AppResult<HashMap<String, UserRow>> {
    let mut users: HashMap<String, UserRow> = HashMap::new();
    let Some(connection) = open_existing(path, Kind::User)? else { return Ok(users) };
    connection.execute_batch("BEGIN")?;
    let result = (|| -> AppResult<()> {
        {
            let mut statement = connection.prepare("SELECT asset_id, favorite, preview_asset, filesystem_path, thumb_error, provenance FROM user_assets")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                users.insert(row.get(0)?, UserRow {
                    favorite: row.get::<_, i64>(1)? != 0,
                    preview: row.get(2)?,
                    filesystem_path: row.get(3)?,
                    thumb: row.get(4)?,
                    flags: row.get(5)?,
                    ..Default::default()
                });
            }
        }
        for (table, column) in [("user_tags", "tag"), ("user_collections", "name")] {
            let mut statement = connection.prepare(&format!("SELECT asset_id, {column} FROM {table} ORDER BY asset_id, ord"))?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let id: String = row.get(0)?;
                let value: String = row.get(1)?;
                let entry = users.entry(id).or_default();
                if table == "user_tags" { entry.tags.push(value) } else { entry.collections.push(value) }
            }
        }
        Ok(())
    })();
    let _ = connection.execute_batch("COMMIT");
    result.map(|_| users)
}

/// Put `user` and `thumb` (and the full provenance) back on an asset's JSON.
pub(crate) fn restore(value: &mut Value, users: &HashMap<String, UserRow>) -> AppResult<()> {
    let object = value.as_object_mut().ok_or_else(|| AppError::msg("A stored asset is not an object"))?;
    let id = object.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let public = object.remove("provenance");
    let row = users.get(&id);
    let mut provenance: Option<ImportProvenance> = public.map(serde_json::from_value).transpose()?;
    if let Some(path) = row.and_then(|row| row.filesystem_path.clone()) {
        provenance.get_or_insert_with(ImportProvenance::default).filesystem_path = Some(path);
    }
    let derived = provenance.as_ref().map(ImportProvenance::tags).unwrap_or_default();
    let verbatim = row.is_some_and(|row| row.flags.is_some());
    let stored = row.map(|row| row.tags.clone()).unwrap_or_default();
    let tags = if verbatim { stored } else { derived.into_iter().chain(stored).collect() };
    let user = UserMeta {
        provenance,
        preview_asset_id: row.and_then(|row| row.preview.clone()),
        favorite: row.is_some_and(|row| row.favorite),
        tags,
        collections: row.map(|row| row.collections.clone()).unwrap_or_default(),
    };
    let thumb: ThumbMeta = match row.and_then(|row| row.thumb.as_deref()) {
        Some(text) => serde_json::from_str(text)?,
        None => ThumbMeta::default(),
    };
    object.insert("user".into(), serde_json::to_value(user)?);
    object.insert("thumb".into(), serde_json::to_value(thumb)?);
    if let Some(variants) = object.get_mut("variants").and_then(Value::as_array_mut) {
        for variant in variants {
            restore(variant, users)?;
        }
    }
    Ok(())
}

// ---- library data --------------------------------------------------------------

/// One `assets` row to be written, with the rows hanging off it.
pub(crate) struct RowIn {
    pub(crate) id: String,
    pub(crate) item_id: Option<String>,
    kind: &'static str,
    type_id: String,
    name: String,
    pack: String,
    category: String,
    format: String,
    source: String,
    publisher: String,
    license: String,
    hash_key: Option<String>,
    content_hash: Option<String>,
    blob: String,
    tags: Vec<String>,
    rel_path: String,
    /// The id of the model a clip was cut from (empty for anything else).
    clip_source: String,
    /// The plugin that imported the asset (empty when none is recorded).
    plugin: String,
    /// `origin.derivedFrom`: the id of the asset this one was made from (empty when none).
    derived_from: String,
    /// The ids of the asset's variants (their personal rows go with it).
    pub(crate) variant_ids: Vec<String>,
    /// Library-relative files of the asset and its variants; `true` for kept source files.
    files: Vec<(String, bool)>,
    rig_ids: Vec<String>,
    /// Storage keys whose content hash the asset (or a variant) shows; empty in a replica.
    hash_keys: Vec<String>,
}

fn kind_of(content: &AssetContent) -> &'static str {
    match content {
        AssetContent::Model { .. } => "model",
        AssetContent::Material { .. } => "material",
        AssetContent::Animation { .. } => "animation",
        AssetContent::Audio { .. } => "audio",
        AssetContent::Plugin { .. } => "plugin",
    }
}

/// The asset's JSON for `assets.asset`; collects the personal rows of it and of its variants.
fn library_value(asset: &Asset, keep_hash: bool, users: &mut BTreeMap<String, UserRow>) -> AppResult<Value> {
    let mut value = serde_json::to_value(asset)?;
    strip(&mut value, asset, keep_hash, users)?;
    Ok(value)
}

fn strip(value: &mut Value, asset: &Asset, keep_hash: bool, users: &mut BTreeMap<String, UserRow>) -> AppResult<()> {
    if let Some(row) = user_row(asset)? {
        if users.insert(asset.id.clone(), row).is_some() {
            return Err(AppError::msg(format!("The catalog lists '{}' twice", asset.id)));
        }
    } else if users.contains_key(&asset.id) {
        return Err(AppError::msg(format!("The catalog lists '{}' twice", asset.id)));
    }
    let object = value.as_object_mut().ok_or_else(|| AppError::msg("An asset is not an object"))?;
    object.remove("user");
    object.remove("thumb");
    if !keep_hash {
        object.remove("contentHash");
    }
    if let Some(provenance) = &asset.user.provenance {
        let mut public = provenance.clone();
        public.filesystem_path = None;
        object.insert("provenance".into(), serde_json::to_value(public)?);
    }
    if let Some(variants) = object.get_mut("variants").and_then(Value::as_array_mut) {
        for (variant, typed) in variants.iter_mut().zip(&asset.variants) {
            strip(variant, typed, keep_hash, users)?;
        }
    }
    Ok(())
}

/// The file format an asset was imported from, as the window shows it (`assetFormat` in
/// `domain/facets.ts`): the recorded format, else the extension of the primary file.
pub(crate) fn derived_format(asset: &Asset) -> String {
    if let Some(recorded) = asset.format.as_deref().and_then(crate::domain::catalog_model::normalize_format) {
        return recorded;
    }
    fn extension(file: &str) -> String {
        let name = file.rsplit(['/', '\\']).next().unwrap_or("");
        match name.rfind('.') {
            Some(dot) if dot > 0 && dot < name.len() - 1 => name[dot + 1..].to_lowercase(),
            _ => String::new(),
        }
    }
    let first = |own: String, fallback: &str| if own.is_empty() { extension(fallback) } else { own };
    match &asset.content {
        AssetContent::Model { fileset, .. } | AssetContent::Animation { fileset, .. } => first(extension(&fileset.gltf), &asset.file_name),
        AssetContent::Audio { audio } => first(extension(&audio.file), &asset.file_name),
        AssetContent::Material { material } => {
            let origin = asset.origin.as_ref().and_then(|origin| origin.files.first()).map(|file| extension(file)).unwrap_or_default();
            first(origin, material.maps.first().map(|map| map.file.as_str()).unwrap_or(""))
        }
        AssetContent::Plugin { files, .. } => extension(files.first().map(String::as_str).unwrap_or("")),
    }
}

/// A provenance field as the window shows it (`provenanceValue` in `domain/facets.ts`): the recorded
/// value, empty when unknown. (The window also reads a personal tag `source: x` when nothing is recorded;
/// personal tags are not library data, so the column does not.)
pub(crate) fn provenance_value(_asset: &Asset, recorded: &Option<String>, _field: &str) -> String {
    recorded.as_deref().map(str::trim).unwrap_or_default().to_string()
}

fn collect_rig_ids(asset: &Asset, out: &mut Vec<String>) {
    for binding in &asset.rigs {
        if !out.contains(&binding.rig_id) {
            out.push(binding.rig_id.clone());
        }
    }
    for variant in &asset.variants {
        collect_rig_ids(variant, out);
    }
}

fn collect_hash_keys(asset: &Asset, out: &mut Vec<String>) {
    if let Some(key) = crate::ops::content_hash::storage_key(asset) {
        if !out.contains(&key) {
            out.push(key);
        }
    }
    for variant in &asset.variants {
        collect_hash_keys(variant, out);
    }
}

fn collect_variant_ids(asset: &Asset, out: &mut Vec<String>) {
    for variant in &asset.variants {
        out.push(variant.id.clone());
        collect_variant_ids(variant, out);
    }
}

/// The row of one catalog asset. `item_id` is the manifest item it is derived from;
/// `content_hash` the fingerprint of its storage folder when the library has one.
pub(crate) fn row_in(asset: &Asset, item_id: Option<String>, content_hash: Option<String>, remote: bool, users: &mut BTreeMap<String, UserRow>) -> AppResult<RowIn> {
    let blob = library_value(asset, remote, users)?;
    let public = asset.user.provenance.clone().unwrap_or_default();
    let hash_key = crate::ops::content_hash::storage_key(asset);
    let content_hash = if remote { asset.content_hash.clone() } else { content_hash };
    let mut content = Vec::new();
    asset.collect_content_files(&mut content);
    let mut all = Vec::new();
    asset.collect_files(&mut all);
    let mut files: Vec<(String, bool)> = Vec::with_capacity(all.len());
    for path in content.iter() {
        if !files.iter().any(|(seen, _)| seen == path) {
            files.push((path.clone(), false));
        }
    }
    for path in all {
        if !files.iter().any(|(seen, _)| *seen == path) {
            files.push((path, true));
        }
    }
    let mut rig_ids = Vec::new();
    collect_rig_ids(asset, &mut rig_ids);
    let mut variant_ids = Vec::new();
    collect_variant_ids(asset, &mut variant_ids);
    let mut hash_keys = Vec::new();
    if !remote {
        collect_hash_keys(asset, &mut hash_keys);
    }
    Ok(RowIn {
        id: asset.id.clone(),
        item_id,
        kind: kind_of(&asset.content),
        type_id: asset.content.type_id().to_string(),
        name: asset.name.clone(),
        pack: asset.pack.clone(),
        category: asset.category.clone(),
        format: derived_format(asset),
        source: provenance_value(asset, &public.source, "source"),
        publisher: provenance_value(asset, &public.publisher, "publisher"),
        license: provenance_value(asset, &public.license, "license"),
        plugin: asset.origin.as_ref().map(|origin| origin.plugin.clone()).unwrap_or_default(),
        derived_from: asset.origin.as_ref().and_then(|origin| origin.derived_from.clone()).unwrap_or_default(),
        hash_key,
        content_hash,
        blob: serde_json::to_string(&blob)?,
        tags: asset.tags.clone(),
        rel_path: asset.rel_path.clone(),
        clip_source: match &asset.content {
            AssetContent::Animation { clip, .. } => clip.source_id.clone(),
            _ => String::new(),
        },
        variant_ids,
        files,
        rig_ids,
        hash_keys,
    })
}

/// The asset row as it is stored, for comparing with the row to write.
pub(crate) struct Old {
    pub(crate) rowid: i64,
    pub(crate) ord: i64,
    pub(crate) blob: String,
    hash_key: Option<String>,
    content_hash: Option<String>,
}

/// The stored row of asset `id`.
pub(crate) fn old_by_id(tx: &Connection, id: &str) -> AppResult<Option<Old>> {
    use rusqlite::OptionalExtension;
    Ok(tx
        .query_row("SELECT rowid, ord, asset, hash_key, content_hash FROM assets WHERE id=?1", [id], |row| {
            Ok(Old { rowid: row.get(0)?, ord: row.get(1)?, blob: row.get(2)?, hash_key: row.get(3)?, content_hash: row.get(4)? })
        })
        .optional()?)
}

/// Remove one asset row and everything hanging off it (tags, files and rig uses
/// by cascade, the search row by hand) and record the deletion.
pub(crate) fn delete_asset_row(tx: &Connection, id: &str, rowid: i64, revision: i64, fts: bool) -> AppResult<()> {
    if fts {
        tx.execute("DELETE FROM asset_search WHERE rowid=?1", [rowid])?;
    }
    tx.execute("DELETE FROM assets WHERE id=?1", [id])?;
    tx.execute("INSERT OR REPLACE INTO changes(revision,asset_id,deleted) VALUES(?1,?2,1)", params![revision, id])?;
    Ok(())
}

/// Insert or update one asset row (with its tags, search row, files and rig
/// uses). A row that is equal to the stored one is left alone except for its
/// place in the order. True if the library changed. `hash_ok`: the `hashes` table
/// is valid under the running scheme, so a key that is in it counts as hashed.
pub(crate) fn write_row(tx: &Transaction, row: &RowIn, old: Option<&Old>, ord: i64, revision: i64, hash_ok: bool, fts: bool) -> AppResult<bool> {
    let rowid = match old {
        None => {
            tx.prepare_cached("INSERT INTO assets(id,item_id,ord,kind,type,name,pack,category,format,source,publisher,license,content_hash,hash_key,asset,revision,rel_path,thumb_name,clip_source,plugin,derived_from) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)")?
                .execute(params![row.id, row.item_id, ord, row.kind, row.type_id, row.name, row.pack, row.category, row.format, row.source, row.publisher, row.license, row.content_hash, row.hash_key, row.blob, revision, row.rel_path, crate::infra::appdata::sanitize_id(&row.id), row.clip_source, row.plugin, row.derived_from])?;
            tx.last_insert_rowid()
        }
        Some(previous) => {
            let same = previous.blob == row.blob && previous.hash_key == row.hash_key && previous.content_hash == row.content_hash;
            if same {
                if previous.ord != ord {
                    tx.prepare_cached("UPDATE assets SET ord=?2, revision=?3 WHERE id=?1")?.execute(params![row.id, ord, revision])?;
                    tx.execute("INSERT OR REPLACE INTO changes(revision,asset_id,deleted) VALUES(?1,?2,0)", params![revision, row.id])?;
                    return Ok(true);
                }
                return Ok(false);
            }
            tx.prepare_cached("UPDATE assets SET item_id=?2,ord=?3,kind=?4,type=?5,name=?6,pack=?7,category=?8,format=?9,source=?10,publisher=?11,license=?12,content_hash=?13,hash_key=?14,asset=?15,revision=?16,rel_path=?17,thumb_name=?18,clip_source=?19,plugin=?20,derived_from=?21 WHERE id=?1")?
                .execute(params![row.id, row.item_id, ord, row.kind, row.type_id, row.name, row.pack, row.category, row.format, row.source, row.publisher, row.license, row.content_hash, row.hash_key, row.blob, revision, row.rel_path, crate::infra::appdata::sanitize_id(&row.id), row.clip_source, row.plugin, row.derived_from])?;
            tx.execute("DELETE FROM asset_tags WHERE asset_id=?1", [&row.id])?;
            tx.execute("DELETE FROM asset_files WHERE asset_id=?1", [&row.id])?;
            tx.execute("DELETE FROM rig_uses WHERE asset_id=?1", [&row.id])?;
            tx.execute("DELETE FROM asset_hash_keys WHERE asset_id=?1", [&row.id])?;
            if fts {
                tx.execute("DELETE FROM asset_search WHERE rowid=?1", [previous.rowid])?;
            }
            previous.rowid
        }
    };
    let mut tag_insert = tx.prepare_cached("INSERT OR IGNORE INTO asset_tags(asset_id,tag,type) VALUES(?1,?2,?3)")?;
    for tag in &row.tags {
        tag_insert.execute(params![row.id, tag, row.type_id])?;
    }
    let mut file_insert = tx.prepare_cached("INSERT OR IGNORE INTO asset_files(path,asset_id,origin) VALUES(?1,?2,?3)")?;
    for (path, origin) in &row.files {
        file_insert.execute(params![path, row.id, *origin as i64])?;
    }
    let mut rig_insert = tx.prepare_cached("INSERT OR IGNORE INTO rig_uses(rig_id,asset_id) VALUES(?1,?2)")?;
    for rig in &row.rig_ids {
        rig_insert.execute(params![rig, row.id])?;
    }
    let mut key_insert = tx.prepare_cached("INSERT OR IGNORE INTO asset_hash_keys(hash_key,asset_id,hashed) VALUES(?1,?2, ?3 AND EXISTS (SELECT 1 FROM hashes WHERE key=?1))")?;
    for key in &row.hash_keys {
        key_insert.execute(params![key, row.id, hash_ok as i64])?;
    }
    if fts {
        tx.execute("INSERT INTO asset_search(rowid,name,tags) VALUES(?1,?2,?3)", params![rowid, row.name, row.tags.join(" ")])?;
    }
    tx.execute("INSERT OR REPLACE INTO changes(revision,asset_id,deleted) VALUES(?1,?2,0)", params![revision, row.id])?;
    Ok(true)
}

/// `INSERT OR REPLACE` of one rig with its lookup columns.
pub(crate) fn write_rig(tx: &Connection, rig: &crate::domain::rig::Rig, replace: bool) -> AppResult<()> {
    let sql = if replace { "INSERT OR REPLACE INTO rigs(id,rig,kind,shape) VALUES(?1,?2,?3,?4)" } else { "INSERT OR IGNORE INTO rigs(id,rig,kind,shape) VALUES(?1,?2,?3,?4)" };
    tx.prepare_cached(sql)?.execute(params![rig.id, serde_json::to_string(rig)?, rig.kind, crate::domain::rig::shape(rig)])?;
    Ok(())
}

/// Write `catalog` into the open transactions; true if library rows changed.
/// (A change that only touches `user.db` is not a library change.)
fn write_catalog(tx: &Transaction, user_tx: &Transaction, catalog: &Catalog, remote: bool, revision: i64) -> AppResult<bool> {
    let hash_table = if remote { HashMap::new() } else { hashes::load_map(tx)? };
    let hash_ok = !remote && hashes::scheme_ok(tx)?;
    let item_ids: HashSet<String> = {
        let mut statement = tx.prepare("SELECT id FROM items")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    };

    let mut users: BTreeMap<String, UserRow> = BTreeMap::new();
    let mut rows: Vec<RowIn> = Vec::with_capacity(catalog.assets.len());
    let mut seen = HashSet::new();
    for asset in &catalog.assets {
        if !seen.insert(asset.id.as_str()) {
            return Err(AppError::msg(format!("The catalog lists '{}' twice", asset.id)));
        }
        let item_id = if item_ids.contains(&asset.id) {
            Some(asset.id.clone())
        } else if let AssetContent::Animation { clip, .. } = &asset.content {
            item_ids.contains(&clip.source_id).then(|| clip.source_id.clone())
        } else {
            None
        };
        let content_hash = crate::ops::content_hash::storage_key(asset).and_then(|key| hash_table.get(&key).cloned());
        rows.push(row_in(asset, item_id, content_hash, remote, &mut users)?);
    }

    let mut old: HashMap<String, Old> = HashMap::new();
    {
        let mut statement = tx.prepare("SELECT id, rowid, ord, asset, hash_key, content_hash FROM assets")?;
        let mut found = statement.query([])?;
        while let Some(row) = found.next()? {
            old.insert(row.get(0)?, Old { rowid: row.get(1)?, ord: row.get(2)?, blob: row.get(3)?, hash_key: row.get(4)?, content_hash: row.get(5)? });
        }
    }
    let ords = assign_order(&old, &rows);
    let mut changed = false;

    let wanted: HashSet<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    for (id, previous) in &old {
        if !wanted.contains(id.as_str()) {
            delete_asset_row(tx, id, previous.rowid, revision, true)?;
            changed = true;
        }
    }
    for (row, ord) in rows.iter().zip(&ords) {
        changed |= write_row(tx, row, old.get(&row.id), *ord, revision, hash_ok, true)?;
    }

    // Rigs: a handful, so any difference rewrites them all.
    let wanted_rigs: Vec<(String, String)> = catalog.rigs.iter().map(|rig| Ok((rig.id.clone(), serde_json::to_string(rig)?))).collect::<AppResult<_>>()?;
    let current_rigs: Vec<(String, String)> = {
        let mut statement = tx.prepare("SELECT id, rig FROM rigs ORDER BY rowid")?;
        let found = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        found.collect::<Result<_, _>>()?
    };
    if wanted_rigs != current_rigs {
        tx.execute("DELETE FROM rigs", [])?;
        for rig in &catalog.rigs {
            write_rig(tx, rig, true)?;
        }
        changed = true;
    }

    write_users(user_tx, &users)?;
    Ok(changed)
}

/// Where each row goes in the order. Rows that were there keep their number (a
/// removal leaves a gap, new rows go to the end); only a reorder renumbers.
fn assign_order(old: &HashMap<String, Old>, rows: &[RowIn]) -> Vec<i64> {
    let mut old_order: Vec<(&str, i64)> = old.iter().map(|(id, row)| (id.as_str(), row.ord)).collect();
    old_order.sort_by_key(|(id, ord)| (*ord, *id));
    let wanted: HashSet<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    let kept_old: Vec<&str> = old_order.iter().map(|(id, _)| *id).filter(|id| wanted.contains(id)).collect();
    let kept_new: Vec<&str> = rows.iter().map(|row| row.id.as_str()).filter(|id| old.contains_key(*id)).collect();
    let first_new = rows.iter().position(|row| !old.contains_key(&row.id)).unwrap_or(rows.len());
    let appended_only = rows[first_new..].iter().all(|row| !old.contains_key(&row.id));
    if kept_old == kept_new && appended_only {
        let mut next = old_order.iter().map(|(_, ord)| *ord).max().map_or(0, |ord| ord + 1);
        rows.iter().map(|row| match old.get(&row.id) {
            Some(previous) => previous.ord,
            None => {
                next += 1;
                next - 1
            }
        }).collect()
    } else {
        (0..rows.len() as i64).collect()
    }
}

/// The personal rows of `ids`, read in groups.
pub(crate) fn read_users(connection: &Connection, ids: &[String]) -> AppResult<HashMap<String, UserRow>> {
    let mut users: HashMap<String, UserRow> = HashMap::new();
    for chunk in ids.chunks(400) {
        let marks = vec!["?"; chunk.len()].join(",");
        {
            let mut statement = connection.prepare(&format!("SELECT asset_id, favorite, preview_asset, filesystem_path, thumb_error, provenance FROM user_assets WHERE asset_id IN ({marks})"))?;
            let mut rows = statement.query(rusqlite::params_from_iter(chunk.iter()))?;
            while let Some(row) = rows.next()? {
                users.insert(row.get(0)?, UserRow { favorite: row.get::<_, i64>(1)? != 0, preview: row.get(2)?, filesystem_path: row.get(3)?, thumb: row.get(4)?, flags: row.get(5)?, ..Default::default() });
            }
        }
        for (table, column) in [("user_tags", "tag"), ("user_collections", "name")] {
            let mut statement = connection.prepare(&format!("SELECT asset_id, {column} FROM {table} WHERE asset_id IN ({marks}) ORDER BY asset_id, ord"))?;
            let mut rows = statement.query(rusqlite::params_from_iter(chunk.iter()))?;
            while let Some(row) = rows.next()? {
                let id: String = row.get(0)?;
                let value: String = row.get(1)?;
                let entry = users.entry(id).or_default();
                if table == "user_tags" { entry.tags.push(value) } else { entry.collections.push(value) }
            }
        }
    }
    Ok(users)
}

/// Make the personal rows of `desired` what they say, and remove the rows of
/// `forget` that `desired` does not name. Only these ids are read or written.
pub(crate) fn write_users_for(tx: &Transaction, desired: &BTreeMap<String, UserRow>, forget: &[String]) -> AppResult<()> {
    let mut ids: Vec<String> = desired.keys().cloned().collect();
    ids.extend(forget.iter().filter(|id| !desired.contains_key(*id)).cloned());
    let existing = read_users(tx, &ids)?;
    for id in forget.iter().filter(|id| !desired.contains_key(*id)) {
        if existing.contains_key(id) {
            for table in ["user_assets", "user_tags", "user_collections"] {
                tx.execute(&format!("DELETE FROM {table} WHERE asset_id=?1"), [id])?;
            }
        }
    }
    put_user_rows(tx, desired, &existing)
}

fn put_user_rows(tx: &Transaction, desired: &BTreeMap<String, UserRow>, existing: &HashMap<String, UserRow>) -> AppResult<()> {
    for (id, row) in desired {
        if existing.get(id) == Some(row) {
            continue;
        }
        tx.execute(
            "INSERT INTO user_assets(asset_id,favorite,preview_asset,filesystem_path,thumb_error,provenance) VALUES(?1,?2,?3,?4,?5,?6) \
             ON CONFLICT(asset_id) DO UPDATE SET favorite=excluded.favorite, preview_asset=excluded.preview_asset, filesystem_path=excluded.filesystem_path, thumb_error=excluded.thumb_error, provenance=excluded.provenance",
            params![id, row.favorite as i64, row.preview, row.filesystem_path, row.thumb, row.flags],
        )?;
        tx.execute("DELETE FROM user_tags WHERE asset_id=?1", [id])?;
        tx.execute("DELETE FROM user_collections WHERE asset_id=?1", [id])?;
        for (ord, tag) in row.tags.iter().enumerate() {
            // A duplicate tag cannot be stored twice; the first one wins.
            tx.execute("INSERT OR IGNORE INTO user_tags(asset_id,tag,ord) VALUES(?1,?2,?3)", params![id, tag, ord as i64])?;
        }
        for (ord, name) in row.collections.iter().enumerate() {
            tx.execute("INSERT OR IGNORE INTO user_collections(asset_id,name,ord) VALUES(?1,?2,?3)", params![id, name, ord as i64])?;
        }
    }
    Ok(())
}

fn write_users(tx: &Transaction, desired: &BTreeMap<String, UserRow>) -> AppResult<()> {
    let existing = {
        let mut map: HashMap<String, UserRow> = HashMap::new();
        {
            let mut statement = tx.prepare("SELECT asset_id, favorite, preview_asset, filesystem_path, thumb_error, provenance FROM user_assets")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                map.insert(row.get(0)?, UserRow { favorite: row.get::<_, i64>(1)? != 0, preview: row.get(2)?, filesystem_path: row.get(3)?, thumb: row.get(4)?, flags: row.get(5)?, ..Default::default() });
            }
        }
        for (table, column) in [("user_tags", "tag"), ("user_collections", "name")] {
            let mut statement = tx.prepare(&format!("SELECT asset_id, {column} FROM {table} ORDER BY asset_id, ord"))?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let id: String = row.get(0)?;
                let value: String = row.get(1)?;
                let entry = map.entry(id).or_default();
                if table == "user_tags" { entry.tags.push(value) } else { entry.collections.push(value) }
            }
        }
        map
    };
    for id in existing.keys().filter(|id| !desired.contains_key(*id)) {
        for table in ["user_assets", "user_tags", "user_collections"] {
            tx.execute(&format!("DELETE FROM {table} WHERE asset_id=?1"), [id])?;
        }
    }
    put_user_rows(tx, desired, &existing)?;
    Ok(())
}
