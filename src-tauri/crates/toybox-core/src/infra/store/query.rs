//! Reading a few rows: assets by id, manifest entries by id, who owns an id,
//! which files only some assets use. None of these builds a [`Catalog`]; each
//! reads the rows it is asked about through an index.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::{params_from_iter, Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;

use super::catalog::{self, read_users, restore, UserRow};
use super::{open_existing, open_or_create, Kind, Store};
use crate::domain::catalog_model::{Asset, ThumbMeta, ThumbState};
use crate::domain::rig::Rig;
use crate::error::AppResult;

const CHUNK: usize = 400;

fn marks(count: usize) -> String {
    vec!["?"; count].join(",")
}

fn collect_ids(value: &Value, out: &mut Vec<String>) {
    if let Some(id) = value.get("id").and_then(Value::as_str) {
        out.push(id.to_string());
    }
    for variant in value.get("variants").and_then(Value::as_array).into_iter().flatten() {
        collect_ids(variant, out);
    }
}

fn find_node(value: &Value, id: &str) -> Option<Value> {
    for variant in value.get("variants").and_then(Value::as_array).into_iter().flatten() {
        if variant.get("id").and_then(Value::as_str) == Some(id) {
            return Some(variant.clone());
        }
        if let Some(found) = find_node(variant, id) {
            return Some(found);
        }
    }
    None
}

impl Store {
    /// Run `work` against the library database inside one read transaction; `None`
    /// when the library has no database yet.
    pub(super) fn read<T>(&self, work: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<Option<T>> {
        let Some(connection) = open_existing(&self.library, Kind::Library)? else { return Ok(None) };
        connection.execute_batch("BEGIN")?;
        let result = work(&connection);
        let _ = connection.execute_batch("COMMIT");
        result.map(Some)
    }

    pub(super) fn user_rows(&self, ids: &[String]) -> AppResult<HashMap<String, UserRow>> {
        match open_existing(&self.user, Kind::User)? {
            Some(connection) => {
                connection.execute_batch("BEGIN")?;
                let result = read_users(&connection, ids);
                let _ = connection.execute_batch("COMMIT");
                result
            }
            None => Ok(HashMap::new()),
        }
    }

    /// Turn stored blobs into assets, with their personal data and thumbnail error.
    fn inflate(&self, mut blobs: Vec<Value>) -> AppResult<Vec<Asset>> {
        let mut ids = Vec::new();
        for blob in &blobs {
            collect_ids(blob, &mut ids);
        }
        let users = self.user_rows(&ids)?;
        let mut out = Vec::with_capacity(blobs.len());
        for blob in &mut blobs {
            restore(blob, &users)?;
            out.push(serde_json::from_value::<Asset>(blob.take())?);
        }
        Ok(out)
    }

    /// The assets (top-level items of the catalog or their variants) with these
    /// ids, in the order asked; ids that are not there are left out. Content
    /// hashes are not filled in (see `ops::content_hash::fill_assets`).
    pub fn load_assets(&self, ids: &[String]) -> AppResult<Vec<Asset>> {
        let Some(blobs) = self.read(|library| {
            let mut found: HashMap<String, Value> = HashMap::new();
            for chunk in ids.chunks(CHUNK) {
                let mut statement = library.prepare(&format!("SELECT id, asset FROM assets WHERE id IN ({})", marks(chunk.len())))?;
                let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                while let Some(row) = rows.next()? {
                    let text: String = row.get(1)?;
                    found.insert(row.get(0)?, serde_json::from_str(&text)?);
                }
            }
            // A variant is inside its parent's row.
            let missing: Vec<&String> = ids.iter().filter(|id| !found.contains_key(*id)).collect();
            if !missing.is_empty() {
                let mut owner = library.prepare("SELECT item_id FROM item_variants WHERE variant_id=?1")?;
                let mut rows_of = library.prepare("SELECT asset FROM assets WHERE item_id=?1 OR id=?1")?;
                for id in missing {
                    let owners: Vec<String> = owner.query_map([id], |row| row.get(0))?.collect::<Result<_, _>>()?;
                    'owners: for item in owners {
                        let texts: Vec<String> = rows_of.query_map([&item], |row| row.get(0))?.collect::<Result<_, _>>()?;
                        for text in texts {
                            let blob: Value = serde_json::from_str(&text)?;
                            if let Some(node) = find_node(&blob, id) {
                                found.insert(id.clone(), node);
                                break 'owners;
                            }
                        }
                    }
                }
            }
            let mut ordered = Vec::new();
            let mut seen = HashSet::new();
            for id in ids {
                if seen.insert(id.as_str()) {
                    if let Some(blob) = found.remove(id) {
                        ordered.push(blob);
                    }
                }
            }
            Ok(ordered)
        })?
        else {
            return Ok(Vec::new());
        };
        self.inflate(blobs)
    }

    /// The top-level catalog assets with these ids (variants are not looked for), in
    /// the order asked; ids that are not there are left out.
    pub fn load_top_assets(&self, ids: &[String]) -> AppResult<Vec<Asset>> {
        let Some(blobs) = self.read(|library| {
            let mut found: HashMap<String, Value> = HashMap::new();
            for chunk in ids.chunks(CHUNK) {
                let mut statement = library.prepare(&format!("SELECT id, asset FROM assets WHERE id IN ({})", marks(chunk.len())))?;
                let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                while let Some(row) = rows.next()? {
                    let text: String = row.get(1)?;
                    found.insert(row.get(0)?, serde_json::from_str(&text)?);
                }
            }
            let mut seen = HashSet::new();
            Ok(ids.iter().filter(|id| seen.insert(id.as_str())).filter_map(|id| found.remove(id)).collect::<Vec<Value>>())
        })?
        else {
            return Ok(Vec::new());
        };
        self.inflate(blobs)
    }

    /// One asset or variant.
    pub fn load_asset(&self, id: &str) -> AppResult<Option<Asset>> {
        Ok(self.load_assets(&[id.to_string()])?.into_iter().next())
    }

    /// The top-level assets derived from these manifest items (an item usually
    /// has one; the clips of an animation source are several), in catalog order.
    pub fn load_item_assets(&self, item_ids: &[String]) -> AppResult<Vec<Asset>> {
        Ok(self.load_item_groups(item_ids)?.into_iter().map(|(_, asset)| asset).collect())
    }

    /// [`Store::load_item_assets`] with the manifest item each asset belongs to.
    pub fn load_item_groups(&self, item_ids: &[String]) -> AppResult<Vec<(String, Asset)>> {
        let Some(found) = self.read(|library| {
            let mut found: Vec<(i64, String, Value)> = Vec::new();
            let mut seen = HashSet::new();
            for chunk in item_ids.chunks(CHUNK) {
                let mut statement = library.prepare(&format!("SELECT ord, COALESCE(item_id, id), asset FROM assets WHERE item_id IN ({0}) OR id IN ({0})", marks(chunk.len())))?;
                let params: Vec<&String> = chunk.iter().chain(chunk.iter()).collect();
                let mut rows = statement.query(params_from_iter(params))?;
                while let Some(row) = rows.next()? {
                    let text: String = row.get(2)?;
                    let blob: Value = serde_json::from_str(&text)?;
                    if seen.insert(blob.get("id").and_then(Value::as_str).unwrap_or("").to_string()) {
                        found.push((row.get(0)?, row.get(1)?, blob));
                    }
                }
            }
            found.sort_by_key(|(ord, _, _)| *ord);
            Ok(found)
        })?
        else {
            return Ok(Vec::new());
        };
        let owners: Vec<String> = found.iter().map(|(_, item, _)| item.clone()).collect();
        let assets = self.inflate(found.into_iter().map(|(_, _, blob)| blob).collect())?;
        Ok(owners.into_iter().zip(assets).collect())
    }

    /// Assets that are `ids` themselves or that sit at one of `rel_paths`: the
    /// rows an incoming asset may absorb. Order of the catalog.
    pub fn load_assets_matching(&self, ids: &[String], rel_paths: &[String]) -> AppResult<Vec<Asset>> {
        let Some(blobs) = self.read(|library| {
            let mut found: Vec<(i64, Value)> = Vec::new();
            let mut seen = HashSet::new();
            let mut push = |ord: i64, text: String| -> AppResult<()> {
                let blob: Value = serde_json::from_str(&text)?;
                if let Some(id) = blob.get("id").and_then(Value::as_str) {
                    if seen.insert(id.to_string()) {
                        found.push((ord, blob));
                    }
                }
                Ok(())
            };
            for (column, values) in [("id", ids), ("rel_path", rel_paths)] {
                for chunk in values.chunks(CHUNK) {
                    let mut statement = library.prepare(&format!("SELECT ord, asset FROM assets WHERE {column} IN ({})", marks(chunk.len())))?;
                    let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                    while let Some(row) = rows.next()? {
                        push(row.get(0)?, row.get(1)?)?;
                    }
                }
            }
            found.sort_by_key(|(ord, _)| *ord);
            Ok(found.into_iter().map(|(_, blob)| blob).collect())
        })?
        else {
            return Ok(Vec::new());
        };
        self.inflate(blobs)
    }

    /// The manifest entries of these items, as stored; ones that are not there are left out.
    pub fn load_entries(&self, ids: &[String]) -> AppResult<Vec<Value>> {
        Ok(self
            .read(|library| {
                let mut found: HashMap<String, Value> = HashMap::new();
                for chunk in ids.chunks(CHUNK) {
                    let mut statement = library.prepare(&format!("SELECT id, entry FROM items WHERE id IN ({})", marks(chunk.len())))?;
                    let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                    while let Some(row) = rows.next()? {
                        let text: String = row.get(1)?;
                        found.insert(row.get(0)?, serde_json::from_str(&text)?);
                    }
                }
                Ok(ids.iter().filter_map(|id| found.remove(id)).collect())
            })?
            .unwrap_or_default())
    }

    /// The manifest entries (id and entry) whose physical key is one of `keys`.
    pub fn load_entries_by_pkey(&self, keys: &[String]) -> AppResult<Vec<(String, Value)>> {
        Ok(self
            .read(|library| {
                let mut found = Vec::new();
                for chunk in keys.chunks(CHUNK) {
                    let mut statement = library.prepare(&format!("SELECT id, entry FROM items WHERE pkey IN ({}) ORDER BY seq", marks(chunk.len())))?;
                    let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                    while let Some(row) = rows.next()? {
                        let text: String = row.get(1)?;
                        found.push((row.get(0)?, serde_json::from_str(&text)?));
                    }
                }
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// For each of `ids` that the manifest uses (as an item or as a variant), the
    /// top-level item that owns it. A top-level id wins over a variant of the same id.
    pub fn id_owners(&self, ids: &[String]) -> AppResult<HashMap<String, String>> {
        Ok(self
            .read(|library| {
                let mut owners: HashMap<String, String> = HashMap::new();
                for chunk in ids.chunks(CHUNK) {
                    let mut statement = library.prepare(&format!("SELECT variant_id, item_id FROM item_variants WHERE variant_id IN ({})", marks(chunk.len())))?;
                    let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                    while let Some(row) = rows.next()? {
                        owners.entry(row.get(0)?).or_insert(row.get(1)?);
                    }
                    let mut statement = library.prepare(&format!("SELECT id FROM items WHERE id IN ({})", marks(chunk.len())))?;
                    let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                    while let Some(row) = rows.next()? {
                        let id: String = row.get(0)?;
                        owners.insert(id.clone(), id);
                    }
                }
                Ok(owners)
            })?
            .unwrap_or_default())
    }

    /// Which of `ids` are manifest items.
    pub fn present_items(&self, ids: &[String]) -> AppResult<HashSet<String>> {
        Ok(self.id_owners(ids)?.into_iter().filter(|(id, owner)| id == owner).map(|(id, _)| id).collect())
    }

    /// The rigs these assets (and their variants) bind to.
    pub fn rigs_for(&self, assets: &[Asset]) -> AppResult<Vec<Rig>> {
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
        self.rigs_by_id(&ids)
    }

    pub fn rigs_by_id(&self, ids: &[String]) -> AppResult<Vec<Rig>> {
        Ok(self
            .read(|library| {
                let mut found: Vec<(i64, Rig)> = Vec::new();
                for chunk in ids.chunks(CHUNK) {
                    let mut statement = library.prepare(&format!("SELECT rowid, rig FROM rigs WHERE id IN ({})", marks(chunk.len())))?;
                    let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                    while let Some(row) = rows.next()? {
                        let text: String = row.get(1)?;
                        found.push((row.get(0)?, serde_json::from_str(&text)?));
                    }
                }
                found.sort_by_key(|(rowid, _)| *rowid);
                Ok(found.into_iter().map(|(_, rig)| rig).collect())
            })?
            .unwrap_or_default())
    }

    /// A stored rig that `rig` is `equivalent` to (same named topology, rest frames
    /// within tolerance), if the library has one.
    pub fn find_equivalent_rig(&self, rig: &Rig) -> AppResult<Option<Rig>> {
        Ok(self
            .read(|library| {
                let mut statement = library.prepare("SELECT rig FROM rigs WHERE shape=?1 ORDER BY rowid")?;
                let mut rows = statement.query([crate::domain::rig::shape(rig)])?;
                while let Some(row) = rows.next()? {
                    let text: String = row.get(0)?;
                    let stored: Rig = serde_json::from_str(&text)?;
                    if crate::domain::rig::equivalent(&stored, rig) {
                        return Ok(Some(stored));
                    }
                }
                Ok(None)
            })?
            .flatten())
    }

    /// Call `visit` with the id of every catalog asset, in catalog order, without
    /// holding them all.
    pub fn for_each_asset_id(&self, visit: &mut dyn FnMut(&str)) -> AppResult<()> {
        self.read(|library| {
            let mut statement = library.prepare("SELECT id FROM assets ORDER BY ord")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let id: String = row.get(0)?;
                visit(&id);
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Up to `limit` catalog asset ids after `after`, in id order, and the id to continue after
    /// (`None` when that was the end). Each page costs what it returns.
    pub fn asset_id_page(&self, after: &str, limit: usize) -> AppResult<(Vec<String>, Option<String>)> {
        let ids: Vec<String> = self
            .read(|library| {
                let mut statement = library.prepare("SELECT id FROM assets WHERE id > ?1 ORDER BY id LIMIT ?2")?;
                let found = statement.query_map(rusqlite::params![after, limit as i64], |row| row.get::<_, String>(0))?;
                Ok(found.collect::<Result<Vec<_>, _>>()?)
            })?
            .unwrap_or_default();
        let next = if ids.len() >= limit { ids.last().cloned() } else { None };
        Ok((ids, next))
    }

    /// Every rig of the library, in the order they were added (a handful).
    pub fn all_rigs(&self) -> AppResult<Vec<Rig>> {
        Ok(self
            .read(|library| {
                let mut statement = library.prepare("SELECT rig FROM rigs ORDER BY rowid")?;
                let mut rows = statement.query([])?;
                let mut found = Vec::new();
                while let Some(row) = rows.next()? {
                    let text: String = row.get(0)?;
                    found.push(serde_json::from_str(&text)?);
                }
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// The models and animations that bind to a rig, without personal data, in catalog order
    /// (what matching an animation to a model reads; found through `rig_uses`).
    pub fn rigged_assets_all(&self) -> AppResult<Vec<Asset>> {
        let Some(blobs) = self.read(|library| {
            let mut statement = library.prepare(
                "SELECT a.asset FROM assets a WHERE a.kind IN ('model','animation') AND EXISTS (SELECT 1 FROM rig_uses u WHERE u.asset_id = a.id) ORDER BY a.ord",
            )?;
            let mut rows = statement.query([])?;
            let mut found: Vec<Value> = Vec::new();
            while let Some(row) = rows.next()? {
                let text: String = row.get(0)?;
                found.push(serde_json::from_str(&text)?);
            }
            Ok(found)
        })?
        else {
            return Ok(Vec::new());
        };
        let none = HashMap::new();
        blobs
            .into_iter()
            .map(|mut blob| {
                restore(&mut blob, &none)?;
                Ok(serde_json::from_value::<Asset>(blob)?)
            })
            .collect()
    }

    /// How many catalog assets there are.
    pub fn asset_count(&self) -> AppResult<i64> {
        Ok(self.read(|library| Ok(library.query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))?))?.unwrap_or(0))
    }

    /// How many manifest items there are.
    pub fn item_count(&self) -> AppResult<i64> {
        Ok(self.read(|library| Ok(library.query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))?))?.unwrap_or(0))
    }

    /// Library-relative files that only the assets `selected` use: they are
    /// referenced by one of them and by no asset outside the set.
    pub fn exclusive_files(&self, selected: &[String]) -> AppResult<Vec<String>> {
        Ok(self
            .read(|library| {
                library.execute_batch("CREATE TEMP TABLE IF NOT EXISTS sel(id TEXT PRIMARY KEY) WITHOUT ROWID; DELETE FROM sel;")?;
                {
                    let mut insert = library.prepare("INSERT OR IGNORE INTO sel(id) VALUES(?1)")?;
                    for id in selected {
                        insert.execute([id])?;
                    }
                }
                let mut statement = library.prepare(
                    "SELECT DISTINCT f.path FROM asset_files f WHERE f.asset_id IN (SELECT id FROM sel) \
                     AND NOT EXISTS (SELECT 1 FROM asset_files g WHERE g.path = f.path AND g.asset_id NOT IN (SELECT id FROM sel)) ORDER BY f.path",
                )?;
                let found = statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
                library.execute_batch("DELETE FROM sel")?;
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// Thumbnail file names (sanitised ids) of the assets `selected` and of their
    /// variants that no other asset or variant uses.
    pub fn exclusive_thumb_names(&self, selected: &[String]) -> AppResult<Vec<String>> {
        Ok(self
            .read(|library| {
                library.execute_batch("CREATE TEMP TABLE IF NOT EXISTS sel(id TEXT PRIMARY KEY) WITHOUT ROWID; DELETE FROM sel;")?;
                {
                    let mut insert = library.prepare("INSERT OR IGNORE INTO sel(id) VALUES(?1)")?;
                    for id in selected {
                        insert.execute([id])?;
                    }
                }
                let mut statement = library.prepare(
                    "SELECT DISTINCT n FROM (SELECT thumb_name AS n FROM assets WHERE id IN (SELECT id FROM sel) \
                       UNION SELECT thumb_name FROM item_variants WHERE item_id IN (SELECT id FROM sel)) \
                     WHERE n <> '' AND NOT EXISTS (SELECT 1 FROM assets WHERE thumb_name = n AND id NOT IN (SELECT id FROM sel)) \
                       AND NOT EXISTS (SELECT 1 FROM item_variants WHERE thumb_name = n AND item_id NOT IN (SELECT id FROM sel)) ORDER BY n",
                )?;
                let found = statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
                library.execute_batch("DELETE FROM sel")?;
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// For each of `paths`, the assets that list it as a file of their content
    /// (kept source files do not count).
    pub fn content_file_owners(&self, paths: &[String]) -> AppResult<HashMap<String, Vec<String>>> {
        Ok(self
            .read(|library| {
                let mut owners: HashMap<String, Vec<String>> = HashMap::new();
                let mut statement = library.prepare("SELECT asset_id FROM asset_files WHERE path=?1 AND origin=0")?;
                for path in paths {
                    let found: Vec<String> = statement.query_map([path], |row| row.get(0))?.collect::<Result<_, _>>()?;
                    owners.insert(path.clone(), found);
                }
                Ok(owners)
            })?
            .unwrap_or_default())
    }

    /// Which of these library-relative files some asset lists.
    pub fn files_in_use(&self, paths: &[String]) -> AppResult<HashSet<String>> {
        Ok(self
            .read(|library| {
                let mut used = HashSet::new();
                let mut statement = library.prepare("SELECT 1 FROM asset_files WHERE path=?1 LIMIT 1")?;
                for path in paths {
                    if statement.query_row([path], |row| row.get::<_, i64>(0)).optional()?.is_some() {
                        used.insert(path.clone());
                    }
                }
                Ok(used)
            })?
            .unwrap_or_default())
    }

    /// Whether any asset lists a file at or below `folder` (a library-relative folder).
    pub fn folder_in_use(&self, folder: &str) -> AppResult<bool> {
        Ok(self
            .read(|library| {
                let low = format!("{folder}/");
                let high = format!("{folder}0"); // '0' is the character after '/'
                let used: Option<i64> = library
                    .query_row("SELECT 1 FROM asset_files WHERE path >= ?1 AND path < ?2 LIMIT 1", [&low, &high], |row| row.get(0))
                    .optional()?;
                Ok(used.is_some())
            })?
            .unwrap_or(false))
    }

    /// Record a thumbnail error (or clear it) for a catalog asset. False when no
    /// such asset is in the catalog. Only the personal database is written.
    pub fn set_thumb(&self, asset_id: &str, thumb: &ThumbMeta) -> AppResult<bool> {
        let exists = self.read(|library| Ok(library.query_row("SELECT 1 FROM assets WHERE id=?1", [asset_id], |row| row.get::<_, i64>(0)).optional()?.is_some()))?.unwrap_or(false);
        if !exists {
            return Ok(false);
        }
        let mut user = open_or_create(&self.user, Kind::User)?;
        let tx = user.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut row = read_users(&tx, &[asset_id.to_string()])?.remove(asset_id).unwrap_or_default();
        row.thumb = if matches!(thumb.state, ThumbState::Error) || thumb.error.is_some() { Some(serde_json::to_string(thumb)?) } else { None };
        if row == UserRow::default() {
            catalog::write_users_for(&tx, &BTreeMap::new(), &[asset_id.to_string()])?;
        } else {
            catalog::write_users_for(&tx, &BTreeMap::from([(asset_id.to_string(), row)]), &[])?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Change the personal data of these items (or variants) and nothing else: `change` edits the
    /// row of each, and a row left with nothing is forgotten. Only `user.db` is written, in one
    /// transaction; the library revision does not move.
    pub(crate) fn edit_users(&self, ids: &[String], mut change: impl FnMut(&str, &mut UserRow)) -> AppResult<()> {
        let mut user = open_or_create(&self.user, Kind::User)?;
        let tx = user.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for chunk in ids.chunks(CHUNK) {
            let mut rows = read_users(&tx, chunk)?;
            let mut desired: BTreeMap<String, UserRow> = BTreeMap::new();
            let mut forget: Vec<String> = Vec::new();
            for id in chunk {
                let mut row = rows.remove(id).unwrap_or_default();
                change(id, &mut row);
                if row == UserRow::default() { forget.push(id.clone()) } else { desired.insert(id.clone(), row); }
            }
            catalog::write_users_for(&tx, &desired, &forget)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Mark these items as favourites (or not).
    pub fn set_favorite(&self, ids: &[String], favorite: bool) -> AppResult<()> {
        self.edit_users(ids, |_, row| row.favorite = favorite)
    }

    /// Put these items into the collection `name` (or take them out).
    pub fn set_collection_member(&self, ids: &[String], name: &str, member: bool) -> AppResult<()> {
        self.edit_users(ids, |_, row| {
            row.collections.retain(|existing| existing != name);
            if member {
                row.collections.push(name.to_string());
            }
        })
    }

    /// Choose the asset an item is previewed on (`None` clears the choice).
    pub fn set_preview_choice(&self, id: &str, choice: Option<&str>) -> AppResult<()> {
        self.edit_users(&[id.to_string()], |_, row| row.preview = choice.map(str::to_string))
    }

    /// Take personal tags off items (they were moved into the library as library tags): `(id, tags)` pairs.
    pub fn drop_personal_tags(&self, removals: &[(String, Vec<String>)]) -> AppResult<()> {
        let by_id: HashMap<&str, &Vec<String>> = removals.iter().map(|(id, tags)| (id.as_str(), tags)).collect();
        let ids: Vec<String> = removals.iter().map(|(id, _)| id.clone()).collect();
        self.edit_users(&ids, |id, row| {
            if let Some(tags) = by_id.get(id) {
                row.tags.retain(|tag| !tags.contains(tag));
            }
        })
    }

    /// Up to `limit` top-level assets that carry personal tags (older versions kept tags only on this computer).
    pub fn assets_with_personal_tags(&self, limit: usize) -> AppResult<Vec<Asset>> {
        let Some(connection) = open_existing(&self.user, Kind::User)? else { return Ok(Vec::new()) };
        let ids: Vec<String> = {
            let mut statement = connection.prepare("SELECT DISTINCT asset_id FROM user_tags LIMIT ?1")?;
            let found = statement.query_map([limit as i64], |row| row.get::<_, String>(0))?;
            found.collect::<Result<_, _>>()?
        };
        self.load_top_assets(&ids)
    }

    /// Clear the preview choice of every asset or variant that points at one of `ids`.
    pub fn clear_previews(&self, ids: &[String]) -> AppResult<()> {
        if open_existing(&self.user, Kind::User)?.is_none() {
            return Ok(());
        }
        let mut user = open_or_create(&self.user, Kind::User)?;
        let tx = user.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for chunk in ids.chunks(CHUNK) {
            let owners: Vec<String> = {
                let mut statement = tx.prepare(&format!("SELECT asset_id FROM user_assets WHERE preview_asset IN ({})", marks(chunk.len())))?;
                let found = statement.query_map(params_from_iter(chunk.iter()), |row| row.get(0))?;
                found.collect::<Result<_, _>>()?
            };
            tx.execute(&format!("UPDATE user_assets SET preview_asset=NULL WHERE preview_asset IN ({})", marks(chunk.len())), params_from_iter(chunk.iter()))?;
            for owner in owners {
                tx.execute(
                    "DELETE FROM user_assets WHERE asset_id=?1 AND favorite=0 AND preview_asset IS NULL AND filesystem_path IS NULL AND thumb_error IS NULL AND provenance IS NULL \
                     AND NOT EXISTS (SELECT 1 FROM user_tags WHERE asset_id=?1) AND NOT EXISTS (SELECT 1 FROM user_collections WHERE asset_id=?1)",
                    [owner],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Which of `ids` are catalog assets (top level).
    pub fn existing_asset_ids(&self, ids: &[String]) -> AppResult<HashSet<String>> {
        Ok(self
            .read(|library| {
                let mut found = HashSet::new();
                for chunk in ids.chunks(CHUNK) {
                    let mut statement = library.prepare(&format!("SELECT id FROM assets WHERE id IN ({})", marks(chunk.len())))?;
                    let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                    while let Some(row) = rows.next()? {
                        found.insert(row.get::<_, String>(0)?);
                    }
                }
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// Ids of the catalog assets of one kind (`animation`, `model`, ...), in catalog order.
    pub fn asset_ids_of_kind(&self, kind: &str) -> AppResult<Vec<String>> {
        Ok(self
            .read(|library| {
                let mut statement = library.prepare("SELECT id FROM assets WHERE kind=?1 ORDER BY ord")?;
                let found = statement.query_map([kind], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// The assets of one kind that bind to a rig of one of these kinds, without
    /// their personal data (enough to match animations to models).
    pub fn rigged_assets(&self, kind: &str, rig_kinds: &[String]) -> AppResult<Vec<Asset>> {
        let Some(blobs) = self.read(|library| {
            let mut found: Vec<(i64, Value)> = Vec::new();
            for chunk in rig_kinds.chunks(CHUNK) {
                let mut statement = library.prepare(&format!(
                    "SELECT DISTINCT a.ord, a.asset FROM assets a JOIN rig_uses u ON u.asset_id = a.id JOIN rigs r ON r.id = u.rig_id WHERE a.kind = ?1 AND r.kind IN ({}) ORDER BY a.ord",
                    marks(chunk.len())
                ))?;
                let mut values: Vec<String> = vec![kind.to_string()];
                values.extend(chunk.iter().cloned());
                let mut rows = statement.query(params_from_iter(values.iter()))?;
                while let Some(row) = rows.next()? {
                    let text: String = row.get(1)?;
                    found.push((row.get(0)?, serde_json::from_str(&text)?));
                }
            }
            found.sort_by_key(|(ord, _)| *ord);
            found.dedup_by(|a, b| a.1.get("id") == b.1.get("id"));
            Ok(found.into_iter().map(|(_, blob)| blob).collect::<Vec<Value>>())
        })?
        else {
            return Ok(Vec::new());
        };
        let none = HashMap::new();
        blobs
            .into_iter()
            .map(|mut blob| {
                restore(&mut blob, &none)?;
                Ok(serde_json::from_value::<Asset>(blob)?)
            })
            .collect()
    }

    /// The first manifest entry of each pack, in manifest order.
    pub fn pack_samples(&self) -> AppResult<Vec<(String, Value)>> {
        Ok(self
            .read(|library| {
                let mut statement = library.prepare("SELECT pack, entry FROM items WHERE seq IN (SELECT MIN(seq) FROM items GROUP BY pack) ORDER BY seq")?;
                let mut rows = statement.query([])?;
                let mut found = Vec::new();
                while let Some(row) = rows.next()? {
                    let text: String = row.get(1)?;
                    found.push((row.get(0)?, serde_json::from_str(&text)?));
                }
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// The packs of the catalog's assets, in order of first appearance.
    pub fn catalog_packs(&self) -> AppResult<Vec<String>> {
        Ok(self
            .read(|library| {
                let mut statement = library.prepare("SELECT pack FROM assets GROUP BY pack ORDER BY MIN(ord)")?;
                let found = statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// Whether the database holds a manifest (a remote replica has none).
    pub fn manifest_present(&self) -> bool {
        self.meta("hasManifest").is_some()
    }

    /// Ids of the animation clips cut from any of these items or variants.
    pub fn clip_ids_cut_from(&self, owners: &[String]) -> AppResult<Vec<String>> {
        Ok(self
            .read(|library| {
                let mut found = Vec::new();
                for chunk in owners.chunks(CHUNK) {
                    let mut statement = library.prepare(&format!("SELECT id FROM assets WHERE kind='animation' AND clip_source IN ({}) ORDER BY ord", marks(chunk.len())))?;
                    let mut rows = statement.query(params_from_iter(chunk.iter()))?;
                    while let Some(row) = rows.next()? {
                        found.push(row.get::<_, String>(0)?);
                    }
                }
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// The kept sources whose path starts with `prefix` and ends with `suffix` (both compared without
    /// case), found through the file-reference index: what is left of a migration of sources is found
    /// without reading every entry.
    pub fn kept_sources_like(&self, prefix: &str, suffix: &str) -> AppResult<Vec<String>> {
        Ok(self
            .read(|library| {
                // A range of the primary key (`path` sorts without case): everything that starts with `prefix`.
                let mut end = prefix.to_string();
                let last = end.pop().map(|c| char::from_u32(c as u32 + 1).unwrap_or(c)).unwrap_or('\u{1}');
                end.push(last);
                let mut statement = library.prepare("SELECT path FROM asset_files WHERE path >= ?1 AND path < ?2 AND origin = 1")?;
                let mut rows = statement.query([prefix, end.as_str()])?;
                let mut found = Vec::new();
                while let Some(row) = rows.next()? {
                    let path: String = row.get(0)?;
                    if path.to_lowercase().ends_with(&suffix.to_lowercase()) && !found.contains(&path) {
                        found.push(path);
                    }
                }
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// Ids of the assets whose `origin.derivedFrom` is `parent` (a raw id of this library), in catalog order.
    pub fn derived_from(&self, parent: &str) -> AppResult<Vec<String>> {
        Ok(self
            .read(|library| {
                let mut statement = library.prepare("SELECT id FROM assets WHERE derived_from = ?1 ORDER BY ord")?;
                let found = statement.query_map([parent], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// The assets that still lack a content hash in the hashes table: those with a
    /// storage key (their own or a variant's) that has none. Personal data is not
    /// read. Found through the `asset_hash_keys` pending index, so the cost is the
    /// size of the work to do. In catalog order.
    pub fn assets_to_hash(&self) -> AppResult<Vec<Asset>> {
        let Some(blobs) = self.read(|library| {
            let mut statement = library.prepare(
                "SELECT a.asset FROM assets a WHERE a.id IN (SELECT DISTINCT asset_id FROM asset_hash_keys WHERE hashed = 0) ORDER BY a.ord",
            )?;
            let mut rows = statement.query([])?;
            let mut found: Vec<Value> = Vec::new();
            while let Some(row) = rows.next()? {
                let text: String = row.get(0)?;
                found.push(serde_json::from_str(&text)?);
            }
            Ok(found)
        })?
        else {
            return Ok(Vec::new());
        };
        let none = HashMap::new();
        blobs
            .into_iter()
            .map(|mut blob| {
                restore(&mut blob, &none)?;
                Ok(serde_json::from_value::<Asset>(blob)?)
            })
            .collect()
    }

    /// [`Store::assets_to_hash`] a page at a time: up to `limit` assets whose id is
    /// after `after`, in id order, and the id to continue after (`None` when there
    /// are no more). Each page costs what it returns.
    pub fn pending_hash_assets(&self, after: &str, limit: usize) -> AppResult<(Vec<Asset>, Option<String>)> {
        let Some((ids, blobs)) = self.read(|library| {
            let ids: Vec<String> = {
                let mut statement = library.prepare("SELECT DISTINCT asset_id FROM asset_hash_keys WHERE hashed = 0 AND asset_id > ?1 ORDER BY asset_id LIMIT ?2")?;
                let found = statement.query_map(rusqlite::params![after, limit as i64], |row| row.get::<_, String>(0))?;
                found.collect::<Result<_, _>>()?
            };
            let mut blobs: Vec<Value> = Vec::new();
            let mut statement = library.prepare("SELECT asset FROM assets WHERE id=?1")?;
            for id in &ids {
                if let Some(text) = statement.query_row([id], |row| row.get::<_, String>(0)).optional()? {
                    blobs.push(serde_json::from_str(&text)?);
                }
            }
            Ok((ids, blobs))
        })?
        else {
            return Ok((Vec::new(), None));
        };
        let none = HashMap::new();
        let assets = blobs
            .into_iter()
            .map(|mut blob| {
                restore(&mut blob, &none)?;
                Ok(serde_json::from_value::<Asset>(blob)?)
            })
            .collect::<AppResult<Vec<_>>>()?;
        let last = if ids.len() >= limit { ids.last().cloned() } else { None };
        Ok((assets, last))
    }

    /// Storage keys the hashes table holds that no asset row uses any more.
    pub fn orphaned_hash_keys(&self) -> AppResult<Vec<String>> {
        Ok(self
            .read(|library| {
                let mut statement = library.prepare("SELECT key FROM hashes WHERE key NOT IN (SELECT hash_key FROM assets WHERE hash_key IS NOT NULL)")?;
                let found = statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
                Ok(found)
            })?
            .unwrap_or_default())
    }

    /// Up to `limit` manifest entries after id `after` that have an `origin`, and the id to continue after
    /// (`None` at the end).
    pub fn entries_page_with_origin(&self, after: &str, limit: usize) -> AppResult<(Vec<Value>, Option<String>)> {
        let rows: Vec<(String, String)> = self
            .read(|library| {
                let mut statement = library.prepare("SELECT id, entry FROM items WHERE id > ?1 AND instr(entry, '\"origin\"') > 0 ORDER BY id LIMIT ?2")?;
                let found = statement.query_map(rusqlite::params![after, limit as i64], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
                Ok(found.collect::<Result<Vec<_>, _>>()?)
            })?
            .unwrap_or_default();
        let next = if rows.len() >= limit { rows.last().map(|(id, _)| id.clone()) } else { None };
        let mut entries = Vec::with_capacity(rows.len());
        for (_, text) in rows {
            entries.push(serde_json::from_str(&text)?);
        }
        Ok((entries, next))
    }

    /// The `meta` value `key` of the library database, if any.
    pub fn meta(&self, key: &str) -> Option<String> {
        self.read(|library| super::meta_get(library, key)).ok().flatten().flatten()
    }

    /// Set a `meta` value (bookkeeping that is not a library change: no revision).
    pub fn set_meta(&self, key: &str, value: &str) -> AppResult<()> {
        let connection = open_or_create(&self.library, Kind::Library)?;
        super::meta_set(&connection, key, value)?;
        Ok(())
    }
}
