//! Editing the metadata that belongs to the library: the `tags` and the
//! `provenance` of manifest entries. This is the one place that rewrites those
//! after an item was imported.
//!
//! The manifest is the source of truth, so an edit rewrites it (under the same
//! storage lock a publication takes, with a timestamped backup beside it) and
//! patches the cache to match, so nothing has to be rescanned. Manifest and
//! cache are written in one database transaction, which is refused if the
//! manifest moved on since it was read. Files are never touched: tags and
//! provenance are not part of an item's storage identity.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::domain::catalog_model::{Asset, AssetContent, Catalog};
use crate::infra::store::{self, Delta, ItemPut, Store};
use crate::domain::provenance::ImportProvenance;
use crate::domain::{seed, tags};
use crate::error::{AppError, AppResult};
#[cfg(test)]
use crate::config;
#[cfg(test)]
use super::catalog::load_catalog_inner;
#[cfg(test)]
use super::scan::scan_library_catalog;
use crate::infra::registry::LibraryHandle;
use crate::infra::{fsio, library as library_rules, storage};


/// Edits one request may carry.
pub const MAX_EDITS: usize = 5000;
/// Longest source, publisher or licence text kept.
const MAX_TEXT: usize = 200;
/// Longest address kept.
const MAX_URL: usize = 2000;
/// Backups of the manifest a metadata edit leaves (the newest are kept).
const KEEP_BACKUPS: usize = 10;
const BACKUP_PREFIX: &str = "catalog-before-metadata-";

/// A partial change of an item's provenance. A field that is absent stays as it
/// is; `null` or an empty string clears it; text sets it. The path the importer
/// read the files from (`filesystemPath`) is private to its computer and cannot
/// be edited here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvenanceEdit {
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub source: Option<Option<String>>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub publisher: Option<Option<String>>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub license: Option<Option<String>>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub url: Option<Option<String>>,
}

/// A field that is in the JSON, even as `null`, is `Some`.
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Option<String>>, D::Error> {
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

/// One item's change. `id` names a top-level item or one of its variants (raw
/// ids of the library). `tags` replaces the item's library tags (an empty list
/// removes them all; they are normalised, see `domain::tags`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetadataEdit {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ProvenanceEdit>,
}

/// Single-line text: whitespace collapsed, control characters dropped.
fn clean_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

fn edited_field(current: &mut Option<String>, edit: &Option<Option<String>>, label: &str, limit: usize, is_url: bool) -> AppResult<()> {
    let Some(change) = edit else { return Ok(()) };
    let text = change.as_deref().map(|raw| if is_url { raw.trim().to_string() } else { clean_text(raw) }).unwrap_or_default();
    if text.chars().count() > limit {
        return Err(AppError::msg(format!("The {label} is too long (at most {limit} characters).")));
    }
    if is_url && !text.is_empty() {
        let lower = text.to_ascii_lowercase();
        if !(lower.starts_with("http://") || lower.starts_with("https://")) || text.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(AppError::msg("The web address must start with http:// or https:// and have no spaces."));
        }
    }
    *current = if text.is_empty() { None } else { Some(text) };
    Ok(())
}

fn apply_provenance(item: &mut Value, edit: &ProvenanceEdit) -> AppResult<()> {
    let mut current: ImportProvenance = match item.get("provenance").filter(|value| !value.is_null()) {
        Some(value) => serde_json::from_value(value.clone())?,
        None => ImportProvenance::default(),
    };
    edited_field(&mut current.source, &edit.source, "source", MAX_TEXT, false)?;
    edited_field(&mut current.publisher, &edit.publisher, "publisher", MAX_TEXT, false)?;
    edited_field(&mut current.license, &edit.license, "license", MAX_TEXT, false)?;
    edited_field(&mut current.url, &edit.url, "web address", MAX_URL, true)?;
    let object = item.as_object_mut().ok_or_else(|| AppError::msg("A manifest entry is not an object"))?;
    match current.normalized() {
        Some(next) => {
            object.insert("provenance".into(), serde_json::to_value(next)?);
        }
        None => {
            object.shift_remove("provenance");
        }
    }
    Ok(())
}

/// Check a request without a library: how many edits there are and that every
/// provenance value is acceptable. A server answers `bad request` for these
/// before it takes the write lock.
pub fn validate_edits(edits: &[MetadataEdit]) -> AppResult<()> {
    if edits.is_empty() {
        return Err(AppError::msg("There is nothing to change."));
    }
    if edits.len() > MAX_EDITS {
        return Err(AppError::msg(format!("At most {MAX_EDITS} items can be changed at once.")));
    }
    for edit in edits {
        if edit.id.is_empty() || edit.id.len() > 512 {
            return Err(AppError::msg("Every change needs the id of an item."));
        }
        if let Some(provenance) = &edit.provenance {
            apply_provenance(&mut serde_json::json!({}), provenance)?;
        }
    }
    Ok(())
}

/// Where each id sits among the manifest entries: the entry, and the variant if it is one.
fn locations(assets: &[Value]) -> HashMap<String, (usize, Option<usize>)> {
    let mut found: HashMap<String, (usize, Option<usize>)> = HashMap::new();
    for (index, entry) in assets.iter().enumerate() {
        if let Some(id) = entry.get("id").and_then(Value::as_str) {
            found.entry(id.to_string()).or_insert((index, None));
        }
        for (position, variant) in entry.get("variants").and_then(Value::as_array).into_iter().flatten().enumerate() {
            if let Some(id) = variant.get("id").and_then(Value::as_str) {
                found.entry(id.to_string()).or_insert((index, Some(position)));
            }
        }
    }
    found
}

/// Apply `edits` to the manifest document. Nothing is applied unless every edit
/// is valid. Returns the indexes of the top-level entries that changed.
pub fn apply_to_manifest(manifest: &mut Value, edits: &[MetadataEdit]) -> AppResult<Vec<usize>> {
    if edits.is_empty() {
        return Err(AppError::msg("There is nothing to change."));
    }
    if edits.len() > MAX_EDITS {
        return Err(AppError::msg(format!("At most {MAX_EDITS} items can be changed at once.")));
    }
    let mut work = manifest.clone();
    let assets = work.get_mut("assets").and_then(Value::as_array_mut).ok_or_else(|| AppError::msg("Library manifest has no assets"))?;
    let mut touched = Vec::new();
    let places = locations(assets);
    for edit in edits {
        let (index, variant) = places.get(&edit.id).copied()
            .ok_or_else(|| AppError::msg(format!("Selected item is no longer in the library: {}", edit.id)))?;
        let entry = &mut assets[index];
        let item = match variant {
            None => entry,
            Some(position) => &mut entry["variants"][position],
        };
        if let Some(list) = &edit.tags {
            let object = item.as_object_mut().ok_or_else(|| AppError::msg("A manifest entry is not an object"))?;
            let normalised = tags::normalize_tags(list);
            if normalised.is_empty() {
                object.shift_remove("tags");
            } else {
                object.insert("tags".into(), Value::from(normalised));
            }
        }
        if let Some(provenance) = &edit.provenance {
            apply_provenance(item, provenance)?;
        }
        if !touched.contains(&index) {
            touched.push(index);
        }
    }
    *manifest = work;
    Ok(touched)
}

/// What the catalog says about one item after an edit.
struct Effective {
    tags: Vec<String>,
    provenance: Option<ImportProvenance>,
}

fn apply_to_asset(asset: &mut Asset, effective: &Effective) {
    asset.tags = effective.tags.clone();
    let previous = asset.user.provenance.clone();
    let old_tags = previous.as_ref().map(ImportProvenance::tags).unwrap_or_default();
    // The path on the importer's computer is personal metadata the cache keeps.
    let mut next = effective.provenance.clone().unwrap_or_default();
    if let Some(path) = previous.and_then(|p| p.filesystem_path) {
        next.filesystem_path = Some(path);
    }
    let next = next.normalized();
    let mut merged = next.as_ref().map(ImportProvenance::tags).unwrap_or_default();
    for tag in std::mem::take(&mut asset.user.tags) {
        if !old_tags.contains(&tag) && !merged.contains(&tag) {
            merged.push(tag);
        }
    }
    asset.user.tags = merged;
    asset.user.provenance = next;
}

fn walk(assets: &mut [Asset], by_id: &HashMap<String, Effective>, clips: &[(String, Effective)]) {
    for asset in assets {
        if let Some(effective) = by_id.get(&asset.id) {
            apply_to_asset(asset, effective);
        } else if let AssetContent::Animation { clip, .. } = &asset.content {
            // A clip that is not its own file carries its source file's metadata.
            if !clip.standalone {
                if let Some((_, effective)) = clips.iter().find(|(rel, _)| *rel == asset.rel_path) {
                    apply_to_asset(asset, effective);
                }
            }
        }
        walk(&mut asset.variants, by_id, clips);
    }
}

/// The items' catalog assets patched to the entries' new tags and provenance.
/// Returns the effective values per id and the clips cut from an animation source.
fn effective_of(entries: &[Value]) -> AppResult<(HashMap<String, Effective>, Vec<(String, Effective)>)> {
    let mut by_id: HashMap<String, Effective> = HashMap::new();
    let mut clips: Vec<(String, Effective)> = Vec::new();
    for entry in entries {
        let parsed = seed::parse_entry(entry)?;
        if matches!(parsed.content, seed::SeedContent::AnimationSource { .. }) && entry["standalone"].as_bool() != Some(true) {
            clips.push((parsed.rel_path.clone(), Effective { tags: parsed.tags.clone(), provenance: parsed.provenance.clone() }));
        }
        for item in std::iter::once(&parsed).chain(parsed.variants.iter()) {
            by_id.insert(item.id.clone(), Effective { tags: item.tags.clone(), provenance: item.provenance.clone() });
        }
    }
    Ok((by_id, clips))
}

fn prune_backups(dir: &std::path::Path) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut backups: Vec<String> = read
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(String::from))
        .filter(|name| name.starts_with(BACKUP_PREFIX) && name.ends_with(".json"))
        .collect();
    // The stamp is a fixed-width-enough nanosecond count: order by length, then text.
    backups.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    let extra = backups.len().saturating_sub(KEEP_BACKUPS);
    for name in backups.into_iter().take(extra) {
        let _ = fsio::remove_file_if_exists(&dir.join(name));
    }
}

/// Change the library tags and provenance of items of a local library and return
/// the top-level items that were edited, as they are afterwards (an edited variant
/// comes inside its parent).
///
/// All or nothing: an unknown id or an invalid value rejects the whole request
/// before anything is written. Only the rows of the edited items are read and
/// written (one transaction, refused if the manifest moved on since it was read);
/// a safety copy of the entries as they were stays beside the database. Files are
/// never touched. A remote library is edited through its server (`toybox-remote`),
/// never here.
pub fn update_entries(handle: &LibraryHandle, edits: &[MetadataEdit]) -> AppResult<Vec<Asset>> {
    if handle.is_remote() {
        return Err(AppError::msg("A remote library is changed through its server"));
    }
    if !library_rules::is_writable(&handle.root) {
        return Err(AppError::msg("This library is read-only, so its tags and sources cannot be changed."));
    }
    if edits.is_empty() {
        return Err(AppError::msg("There is nothing to change."));
    }
    if edits.len() > MAX_EDITS {
        return Err(AppError::msg(format!("At most {MAX_EDITS} items can be changed at once.")));
    }
    let mut trace = crate::infra::trace::Trace::new("edit");
    // Finish any migration or interrupted publication first (this takes the storage lock itself).
    super::catalog::ready_for_write(handle)?;
    trace.phase("open + ready");
    let store = Store::for_handle(handle);
    let mut edited = storage::exclusive(&handle.root, &handle.cache_path, |root| {
        trace.phase("lock + recover");
        let expect = store::manifest_revision(root)?;
        let ids: Vec<String> = edits.iter().map(|edit| edit.id.clone()).collect();
        let owners = store.id_owners(&ids)?;
        let mut item_ids: Vec<String> = Vec::new();
        for edit in edits {
            let owner = owners.get(&edit.id).ok_or_else(|| AppError::msg(format!("Selected item is no longer in the library: {}", edit.id)))?;
            if !item_ids.contains(owner) {
                item_ids.push(owner.clone());
            }
        }
        let old_entries = store.load_entries(&item_ids)?;
        trace.phase("owners + entries");
        if old_entries.len() != item_ids.len() {
            return Err(AppError::msg("Selected item is no longer in the library"));
        }
        let mut doc = serde_json::json!({ "assets": old_entries.clone() });
        apply_to_manifest(&mut doc, edits)?;
        let new_entries: Vec<Value> = doc["assets"].as_array().cloned().unwrap_or_default();
        let (by_id, clips) = effective_of(&new_entries)?;

        trace.phase("apply edits to entries");
        let groups = store.load_item_groups(&item_ids)?;
        trace.phase("load assets");
        let owners_of: Vec<String> = groups.iter().map(|(item, _)| item.clone()).collect();
        let mut assets: Vec<Asset> = groups.into_iter().map(|(_, asset)| asset).collect();
        walk(&mut assets, &by_id, &clips);

        // The safety copy: the entries of the edited items as they were.
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let config_dir = root.join("_library_config");
        fsio::write_bytes(&config_dir.join(format!("{BACKUP_PREFIX}{stamp}.json")), &serde_json::to_vec_pretty(&serde_json::json!({ "assets": old_entries }))?)?;
        let puts: Vec<ItemPut> = item_ids
            .iter()
            .zip(new_entries)
            .map(|(id, entry)| ItemPut {
                id: id.clone(),
                entry: Some(entry),
                assets: owners_of.iter().zip(&assets).filter(|(owner, _)| *owner == id).map(|(_, asset)| asset.clone()).collect(),
                rigs: Vec::new(),
            })
            .collect();
        trace.phase("safety copy");
        // One transaction; refused if the manifest changed since it was read.
        store
            .apply(&Delta { puts, expect_manifest_revision: Some(expect), ..Default::default() })
            .map_err(|error| if error.to_string().contains("changed while it was being edited") { AppError::msg("The library changed while its tags were being saved. Try again.") } else { error })?;
        trace.phase("rows");
        prune_backups(&config_dir);
        trace.phase("prune backups");
        Ok(picked_assets(assets, edits))
    })?;
    super::content_hash::fill_assets(handle, &mut edited);
    Ok(edited)
}

/// The top-level items of `assets` that contain any of the edited ids.
pub fn picked_assets(assets: Vec<Asset>, edits: &[MetadataEdit]) -> Vec<Asset> {
    let ids: std::collections::HashSet<&str> = edits.iter().map(|edit| edit.id.as_str()).collect();
    assets.into_iter().filter(|asset| ids.contains(asset.id.as_str()) || asset.variants.iter().any(|variant| ids.contains(variant.id.as_str()))).collect()
}


/// The top-level items of `catalog` that contain any of the edited ids, as the
/// edit left them (an edited variant is returned inside its parent).
pub fn edited_assets(catalog: &Catalog, edits: &[MetadataEdit]) -> Vec<Asset> {
    let ids: std::collections::HashSet<&str> = edits.iter().map(|edit| edit.id.as_str()).collect();
    catalog
        .assets
        .iter()
        .filter(|asset| ids.contains(asset.id.as_str()) || asset.variants.iter().any(|variant| ids.contains(variant.id.as_str())))
        .cloned()
        .collect()
}

/// Bring a cached catalog of a remote library up to date with items a server
/// answered an edit with (public assets: library tags and provenance without
/// the importer's path). Items the cache does not hold are ignored.
pub fn apply_updated_assets(catalog: &mut Catalog, updated: &[Asset]) {
    patch_assets(&mut catalog.assets, updated);
}

/// [`apply_updated_assets`] for some items alone (the rows of a replica that were read).
pub fn patch_assets(assets: &mut [Asset], updated: &[Asset]) {
    let mut by_id: HashMap<String, Effective> = HashMap::new();
    for asset in updated {
        for item in std::iter::once(asset).chain(asset.variants.iter()) {
            by_id.insert(item.id.clone(), Effective { tags: item.tags.clone(), provenance: item.user.provenance.clone() });
        }
    }
    walk(assets, &by_id, &[]);
}

/// Whether any item of `catalog` (or its variants) still has a personal tag that
/// is not derived from provenance and could be a library tag. Cheap: one pass over memory.
pub fn has_personal_tags(catalog: &Catalog) -> bool {
    fn any(assets: &[Asset]) -> bool {
        assets.iter().any(|asset| asset.user.tags.iter().any(|tag| !tags::is_provenance_tag(tag) && tags::normalize_tag(tag).is_some()) || any(&asset.variants))
    }
    any(&catalog.assets)
}

/// One item's share of a personal-tag migration.
struct Move {
    id: String,
    /// The personal tags (as stored in `user.tags`) that become library tags.
    taken: Vec<String>,
}

fn plan_personal_moves(assets: &[Asset], in_manifest: &dyn Fn(&str) -> bool, edits: &mut Vec<MetadataEdit>, moves: &mut Vec<Move>) {
    for asset in assets {
        let candidates: Vec<&String> = asset.user.tags.iter().filter(|tag| !tags::is_provenance_tag(tag) && tags::normalize_tag(tag).is_some()).collect();
        if !candidates.is_empty() && in_manifest(&asset.id) {
            let next = tags::normalize_tags(asset.tags.iter().chain(candidates.iter().copied()));
            // A tag that did not fit (too many tags) stays personal.
            let taken: Vec<String> = candidates.into_iter().filter(|tag| tags::normalize_tag(tag).is_some_and(|normal| next.contains(&normal))).cloned().collect();
            if !taken.is_empty() {
                if next != asset.tags {
                    edits.push(MetadataEdit { id: asset.id.clone(), tags: Some(next), provenance: None });
                }
                moves.push(Move { id: asset.id.clone(), taken });
            }
        }
        plan_personal_moves(&asset.variants, in_manifest, edits, moves);
    }
}

fn drop_moved(assets: &mut [Asset], done: &HashMap<&str, &Vec<String>>) {
    for asset in assets {
        if let Some(taken) = done.get(asset.id.as_str()) {
            asset.user.tags.retain(|tag| !taken.contains(tag));
        }
        drop_moved(&mut asset.variants, done);
    }
}

/// `meta` key recording that a library holds no personal tag that could still
/// become a library tag. Once set, loading the library does no tag work at all.
pub(crate) const PERSONAL_TAGS_DONE: &str = "personalTagsMigrated";

/// Make the personal tags of a local library's items into library tags.
///
/// There is one kind of tag now: it lives in the library where the item's file
/// is. Tags that older versions kept only in this computer's cache
/// (`user.tags`, apart from the `source: ...`-style tags derived from provenance)
/// are added to the item's library tags and removed from `user.tags`. Nothing is
/// lost: a tag that cannot be a library tag (it has a colon, or the item already
/// has the most tags allowed), an item the manifest does not hold, or a library
/// that cannot be written, leaves the tag where it was. It is idempotent (a tag
/// the item already has as a library tag is only removed from the personal ones),
/// does nothing and writes nothing when there is nothing to move, and never
/// fails: a problem is reported on stderr and the tags stay. Returns how many
/// tags were moved. `catalog` is the cache as it was just read.
pub fn migrate_personal_tags(handle: &LibraryHandle, catalog: &Catalog) -> usize {
    migrate_inner(handle, catalog).0
}

/// [`migrate_personal_tags`], and whether the library is settled afterwards: there
/// was nothing left that could move (so no later load needs to look again).
fn migrate_inner(handle: &LibraryHandle, catalog: &Catalog) -> (usize, bool) {
    if handle.is_remote() || !library_rules::is_writable(&handle.root) {
        return (0, false);
    }
    if !has_personal_tags(catalog) {
        return (0, true);
    }
    let attempt = || -> AppResult<(usize, bool)> {
        let store = Store::for_handle(handle);
        // Which of the items with candidate tags the manifest holds, in one query.
        fn candidates(assets: &[Asset], out: &mut Vec<String>) {
            for asset in assets {
                if asset.user.tags.iter().any(|tag| !tags::is_provenance_tag(tag) && tags::normalize_tag(tag).is_some()) {
                    out.push(asset.id.clone());
                }
                candidates(&asset.variants, out);
            }
        }
        let mut wanted = Vec::new();
        candidates(&catalog.assets, &mut wanted);
        let known = store.id_owners(&wanted)?;
        let (mut edits, mut moves) = (Vec::new(), Vec::new());
        plan_personal_moves(&catalog.assets, &|id| known.contains_key(id), &mut edits, &mut moves);
        if moves.is_empty() {
            return Ok((0, true));
        }
        // The backend takes a limited number of edits per call; a chunk that is refused leaves its items as they were.
        let mut failed: std::collections::HashSet<String> = std::collections::HashSet::new();
        for chunk in edits.chunks(MAX_EDITS) {
            if let Err(error) = update_entries(handle, chunk) {
                eprintln!("Could not move personal tags into the library: {error}");
                failed.extend(chunk.iter().map(|edit| edit.id.clone()));
            }
        }
        let done: HashMap<&str, &Vec<String>> = moves.iter().filter(|m| !failed.contains(&m.id)).map(|m| (m.id.as_str(), &m.taken)).collect();
        if done.is_empty() {
            return Ok((0, false));
        }
        // Take them out of the personal tags of the rows as they are now (the edit patched them).
        let ids: Vec<String> = done.keys().map(|id| id.to_string()).collect();
        let owners = store.id_owners(&ids)?;
        let mut items: Vec<String> = owners.values().cloned().collect();
        items.sort();
        items.dedup();
        let mut groups: Vec<(String, Asset)> = store.load_item_groups(&items)?;
        for (_, asset) in groups.iter_mut() {
            drop_moved(std::slice::from_mut(asset), &done);
        }
        let puts: Vec<ItemPut> = items
            .iter()
            .map(|item| ItemPut { id: item.clone(), entry: None, assets: groups.iter().filter(|(owner, _)| owner == item).map(|(_, asset)| asset.clone()).collect(), rigs: Vec::new() })
            .collect();
        store.apply(&Delta { puts, ..Default::default() })?;
        Ok((done.values().map(|taken| taken.len()).sum(), false))
    };
    match attempt() {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Could not move personal tags into the library: {error}");
            (0, false)
        }
    }
}

/// `load_current_catalog` for a local library of the desktop app: first moves any
/// leftover personal tags into the library (see `migrate_personal_tags`), then
/// reads the catalog. Once a library has nothing left to move that is recorded in
/// its database, and a load does no tag work at all.
pub fn load_catalog_migrating(handle: &LibraryHandle) -> AppResult<Option<Catalog>> {
    let catalog = super::catalog::load_current_catalog(handle)?;
    let Some(read) = catalog else { return Ok(None) };
    if handle.is_remote() {
        return Ok(Some(read));
    }
    let store = Store::for_handle(handle);
    if store.meta(PERSONAL_TAGS_DONE).is_some() {
        return Ok(Some(read));
    }
    let (moved, settled) = migrate_inner(handle, &read);
    if settled {
        let _ = store.set_meta(PERSONAL_TAGS_DONE, "1");
    }
    if moved > 0 {
        return Ok(super::catalog::load_current_catalog(handle).ok().flatten().or(Some(read)));
    }
    Ok(Some(read))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::catalog::save_catalog_inner;
    use crate::infra::registry::LibraryEntry;
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct Base(PathBuf);
    impl Base {
        fn new(name: &str) -> Base {
            let dir = std::env::temp_dir().join(format!("toybox-metadata-{name}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            Base(dir)
        }
    }
    impl Drop for Base {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn library(base: &Path) -> LibraryHandle {
        let root = base.join("root");
        crate::infra::library::ensure_library_folder(&root).unwrap();
        let root = root.canonicalize().unwrap();
        LibraryHandle::local(&base.join("appdata"), &LibraryEntry::local("lib", "lib", &root.to_string_lossy()))
    }

    fn put(library: &LibraryHandle, rel: &str, bytes: &[u8]) {
        fsio::write_bytes(&library.root.join(rel), bytes).unwrap();
    }

    fn put_scene(library: &LibraryHandle, rel: &str, bin: &str) {
        put(library, rel, &serde_json::to_vec(&json!({"asset":{"version":"2.0"},"buffers":[{"uri":bin,"byteLength":4}]})).unwrap());
    }

    fn manifest(library: &LibraryHandle) -> Value {
        store::read_manifest(&library.root).unwrap()
    }

    fn entry(library: &LibraryHandle, id: &str) -> Value {
        manifest(library)["assets"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap()
    }

    /// A model with a variant and a provenance, and a plugin-typed item, both
    /// imported through the real commit path.
    fn seeded(base: &Path) -> LibraryHandle {
        let library = library(base);
        for stem in ["Crate", "Crate_LOD1"] {
            put_scene(&library, &format!("library/_import_s/props/{stem}.gltf"), &format!("{stem}.bin"));
            put(&library, &format!("library/_import_s/props/{stem}.bin"), stem.as_bytes());
        }
        put(&library, "library/_import_s/skybox/Night/px.png", b"px");
        let entries = vec![
            json!({"id":"uid://crate","pack":"_import_s","category":"props","file":"Crate.gltf","tags":["Wood","  Box "],
                "provenance":{"source":"Crate Pack","license":"CC0","filesystemPath":"P:/packs/crate"},
                "variants":[{"id":"uid://crate_lod","pack":"_import_s","category":"props","file":"Crate_LOD1.gltf"}]}),
            json!({"id":"uid://night","kind":"plugin","type":"skybox","pack":"_import_s","category":"night","name":"Night","files":["px.png"],"meta":{}}),
        ];
        super::super::import::merge_entries(&library, entries).unwrap();
        library
    }

    fn edit(id: &str, tags: Option<&[&str]>) -> MetadataEdit {
        MetadataEdit { id: id.into(), tags: tags.map(|t| t.iter().map(|s| s.to_string()).collect()), provenance: None }
    }

    fn catalog_of(library: &LibraryHandle) -> Catalog {
        load_catalog_inner(library).unwrap().unwrap()
    }

    #[test]
    fn manifest_tags_are_normalised_reach_the_catalog_and_survive_a_rescan() {
        let base = Base::new("rescan");
        let library = seeded(&base.0);
        assert_eq!(entry(&library, "uid://crate")["tags"], json!(["wood", "box"]));
        let cached = catalog_of(&library);
        let item = cached.assets.iter().find(|a| a.id == "uid://crate").unwrap();
        assert_eq!(item.tags, vec!["wood", "box"]);
        assert!(item.user.tags.iter().all(|t| !t.contains("wood")), "library tags are not personal tags");
        let json = serde_json::to_value(item).unwrap();
        assert_eq!(json["tags"], json!(["wood", "box"]));
        assert!(json["user"]["tags"].as_array().unwrap().contains(&json!("source: Crate Pack")));
        let rescanned = scan_library_catalog(&library).unwrap();
        assert_eq!(rescanned.assets.iter().find(|a| a.id == "uid://crate").unwrap().tags, vec!["wood", "box"]);
        // An item without tags writes no field at all.
        assert!(entry(&library, "uid://night").get("tags").is_none());
        assert!(serde_json::to_value(rescanned.assets.iter().find(|a| a.id == "uid://night").unwrap()).unwrap().get("tags").is_none());
    }

    #[test]
    fn editing_tags_rewrites_the_manifest_and_patches_the_cache_without_a_rescan() {
        let base = Base::new("tags");
        let library = seeded(&base.0);
        // A personal tag, a favourite and a thumbnail must stay as they are.
        let mut cached = catalog_of(&library);
        for asset in &mut cached.assets {
            if asset.id == "uid://crate" {
                asset.user.favorite = true;
                asset.user.tags.push("mine".into());
            }
        }
        save_catalog_inner(&library, &cached).unwrap();
        let thumb = library.thumb_path("uid://crate");
        fsio::write_bytes(&thumb, b"png").unwrap();
        let files_before: Vec<_> = std::fs::read_dir(library.root.join("library/models")).unwrap().flatten().map(|e| e.file_name()).collect();

        let edits = vec![
            edit("uid://crate", Some(&["Footstep", "footstep", "Loop", "source: fake", ""])),
            edit("uid://crate_lod", Some(&["lod"])),
            edit("uid://night", Some(&["Sky", "night"])),
        ];
        let after = update_entries(&library, &edits).unwrap();

        assert_eq!(entry(&library, "uid://crate")["tags"], json!(["footstep", "loop"]));
        assert_eq!(entry(&library, "uid://crate")["variants"][0]["tags"], json!(["lod"]));
        assert_eq!(entry(&library, "uid://night")["tags"], json!(["sky", "night"]));
        let crate_asset = after.iter().find(|a| a.id == "uid://crate").unwrap();
        assert_eq!(crate_asset.tags, vec!["footstep", "loop"]);
        assert_eq!(crate_asset.variants[0].tags, vec!["lod"]);
        assert!(crate_asset.user.favorite && crate_asset.user.tags.contains(&"mine".to_string()));
        assert!(fsio::exists(&thumb));
        // The cache on disk is the patched one, not a rebuilt one.
        let on_disk = catalog_of(&library);
        assert_eq!(on_disk.assets.iter().find(|a| a.id == "uid://night").unwrap().tags, vec!["sky", "night"]);
        // Files and storage paths did not move.
        let files_after: Vec<_> = std::fs::read_dir(library.root.join("library/models")).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(files_before, files_after);
        // Clearing removes the field.
        update_entries(&library, &[edit("uid://crate", Some(&[]))]).unwrap();
        assert!(entry(&library, "uid://crate").get("tags").is_none());
        assert!(catalog_of(&library).assets.iter().find(|a| a.id == "uid://crate").unwrap().tags.is_empty());
        // Backups of the manifest are kept beside it.
        let backups = std::fs::read_dir(library.root.join("_library_config")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(BACKUP_PREFIX)).count();
        assert_eq!(backups, 2);
    }

    #[test]
    fn editing_tags_keeps_the_storage_path_and_the_fingerprint_key() {
        let base = Base::new("storage");
        let library = seeded(&base.0);
        let before = entry(&library, "uid://crate");
        let night_before = entry(&library, "uid://night");
        update_entries(&library, &[edit("uid://crate", Some(&["other"])), edit("uid://night", Some(&["x"]))]).unwrap();
        let after = entry(&library, "uid://crate");
        assert_eq!(before["storagePath"], after["storagePath"]);
        assert_eq!(before["variants"][0]["storagePath"], after["variants"][0]["storagePath"]);
        assert_eq!(night_before["storagePath"], entry(&library, "uid://night")["storagePath"]);
    }

    #[test]
    fn the_same_item_with_other_tags_is_published_to_the_same_folder() {
        let base = Base::new("samefolder");
        let first = library(&base.0.join("one"));
        let second = library(&base.0.join("two"));
        let mut paths = Vec::new();
        for (library, tags) in [(&first, json!(["a", "b"])), (&second, json!(["completely", "different"]))] {
            put_scene(library, "library/_import_s/props/Rock.gltf", "Rock.bin");
            put(library, "library/_import_s/props/Rock.bin", b"rock");
            super::super::import::merge_entries(library, vec![json!({"id":"uid://rock","pack":"_import_s","category":"props","file":"Rock.gltf","tags":tags})]).unwrap();
            paths.push(entry(library, "uid://rock")["storagePath"].as_str().unwrap().to_string());
        }
        assert_eq!(paths[0], paths[1], "tags are not part of the storage key");
    }

    #[test]
    fn provenance_edits_are_partial_follow_into_the_cache_and_keep_personal_tags() {
        let base = Base::new("provenance");
        let library = seeded(&base.0);
        let mut cached = catalog_of(&library);
        for asset in &mut cached.assets {
            if asset.id == "uid://crate" {
                asset.user.tags.push("mine".into());
            }
        }
        save_catalog_inner(&library, &cached).unwrap();
        let before = catalog_of(&library);
        let crate_before = before.assets.iter().find(|a| a.id == "uid://crate").unwrap();
        assert!(crate_before.user.tags.contains(&"source: Crate Pack".to_string()) && crate_before.user.tags.contains(&"license: CC0".to_string()));

        let change: MetadataEdit = serde_json::from_value(json!({"id":"uid://crate","provenance":{"source":"  Crate   Pack 2 ","publisher":"Acme","url":"https://example.test/crate","license":null}})).unwrap();
        let after = update_entries(&library, &[change]).unwrap();

        let stored = entry(&library, "uid://crate");
        assert_eq!(stored["provenance"]["source"], "Crate Pack 2");
        assert_eq!(stored["provenance"]["publisher"], "Acme");
        assert_eq!(stored["provenance"]["url"], "https://example.test/crate");
        assert!(stored["provenance"].get("license").is_none(), "null clears a field");
        assert_eq!(stored["provenance"]["filesystemPath"], "P:/packs/crate", "fields that were not named stay");
        let asset = after.iter().find(|a| a.id == "uid://crate").unwrap();
        let prov = asset.user.provenance.as_ref().unwrap();
        assert_eq!((prov.source.as_deref(), prov.publisher.as_deref(), prov.license.as_deref()), (Some("Crate Pack 2"), Some("Acme"), None));
        assert_eq!(prov.filesystem_path.as_deref(), Some("P:/packs/crate"));
        assert!(asset.user.tags.contains(&"source: Crate Pack 2".to_string()));
        assert!(asset.user.tags.contains(&"publisher: Acme".to_string()));
        assert!(!asset.user.tags.contains(&"source: Crate Pack".to_string()));
        assert!(!asset.user.tags.iter().any(|t| t.starts_with("license:")));
        assert!(asset.user.tags.contains(&"mine".to_string()), "personal tags stay");
        // The variant has no provenance of its own, so it follows its parent.
        let variant = &asset.variants[0];
        assert_eq!(variant.user.provenance.as_ref().unwrap().source.as_deref(), Some("Crate Pack 2"));
        assert!(variant.user.tags.contains(&"source: Crate Pack 2".to_string()));
        // What a rescan makes of the manifest is what the patched cache says.
        let rescanned = scan_library_catalog(&library).unwrap();
        let again = rescanned.assets.iter().find(|a| a.id == "uid://crate").unwrap();
        assert_eq!(again.user.provenance.as_ref().unwrap().source, prov.source);
        assert!(!again.user.tags.contains(&"source: Crate Pack".to_string()));
        assert!(again.user.tags.contains(&"mine".to_string()));

        // Clearing every field removes the provenance (the importer's path stays).
        let clear: MetadataEdit = serde_json::from_value(json!({"id":"uid://crate","provenance":{"source":"","publisher":"","url":""}})).unwrap();
        update_entries(&library, &[clear]).unwrap();
        assert_eq!(entry(&library, "uid://crate")["provenance"], json!({"filesystemPath":"P:/packs/crate"}));
        let tags_now = catalog_of(&library).assets.iter().find(|a| a.id == "uid://crate").unwrap().user.tags.clone();
        assert!(!tags_now.iter().any(|t| t.contains(": ")) && tags_now.contains(&"mine".to_string()), "{tags_now:?}");
    }

    #[test]
    fn tags_and_provenance_do_not_change_the_content_fingerprint() {
        let base = Base::new("fingerprint");
        let library = seeded(&base.0);
        let hash_of = |library: &LibraryHandle| {
            let catalog = catalog_of(library);
            let mut hasher = crate::ops::content_hash::ContentHasher::new(&library.root);
            catalog.assets.iter().map(|asset| hasher.hash_asset(asset).unwrap()).collect::<Vec<_>>()
        };
        let before = hash_of(&library);
        let change: MetadataEdit = serde_json::from_value(json!({"id":"uid://crate","tags":["x","y"],"provenance":{"source":"Elsewhere"}})).unwrap();
        update_entries(&library, &[change, edit("uid://night", Some(&["sky"]))]).unwrap();
        assert_eq!(before, hash_of(&library));
        assert!(!before.iter().any(|hash| hash.is_empty()));
    }

    #[test]
    fn a_stale_user_save_cannot_put_old_library_tags_back() {
        let base = Base::new("stale");
        let library = seeded(&base.0);
        let stale = catalog_of(&library);
        update_entries(&library, &[edit("uid://crate", Some(&["fresh"])), edit("uid://crate_lod", Some(&["fresh-lod"]))]).unwrap();
        let mut edited = stale.clone();
        for asset in &mut edited.assets { asset.user.favorite = true; }
        super::super::catalog::save_user_catalog(&library, edited).unwrap();
        let now = catalog_of(&library);
        let item = now.assets.iter().find(|a| a.id == "uid://crate").unwrap();
        assert_eq!(item.tags, vec!["fresh"]);
        assert_eq!(item.variants[0].tags, vec!["fresh-lod"]);
        assert!(item.user.favorite, "the personal part of the save still lands");
    }

    #[test]
    fn provenance_is_part_of_the_storage_key_of_a_first_store_and_tags_are_not() {
        let base = Base::new("provkey");
        let mut folders = Vec::new();
        for (index, provenance) in [json!({"source":"A"}), json!({"source":"B"})].into_iter().enumerate() {
            let library = library(&base.0.join(format!("l{index}")));
            put_scene(&library, "library/_import_s/props/Rock.gltf", "Rock.bin");
            put(&library, "library/_import_s/props/Rock.bin", b"rock");
            super::super::import::merge_entries(&library, vec![json!({"id":"uid://rock","pack":"_import_s","category":"props","file":"Rock.gltf","provenance":provenance})]).unwrap();
            folders.push(entry(&library, "uid://rock")["storagePath"].as_str().unwrap().to_string());
        }
        assert_ne!(folders[0], folders[1]);
    }

    #[test]
    fn a_bad_request_changes_nothing() {
        let base = Base::new("atomic");
        let library = seeded(&base.0);
        let manifest_before = manifest(&library);
        let cache_before = serde_json::to_value(Store::for_handle(&library).load_catalog().unwrap().unwrap()).unwrap();
        let stamp_before = Store::for_handle(&library).stamp().unwrap();
        let bad_url: MetadataEdit = serde_json::from_value(json!({"id":"uid://night","provenance":{"url":"javascript:alert(1)"}})).unwrap();
        let too_long: MetadataEdit = serde_json::from_value(json!({"id":"uid://night","provenance":{"source":"x".repeat(500)}})).unwrap();
        for edits in [
            vec![edit("uid://night", Some(&["fine"])), edit("uid://missing", Some(&["x"]))],
            vec![edit("uid://night", Some(&["fine"])), bad_url],
            vec![edit("uid://night", Some(&["fine"])), too_long],
            vec![],
        ] {
            assert!(update_entries(&library, &edits).is_err());
            assert_eq!(manifest(&library), manifest_before);
            assert_eq!(serde_json::to_value(Store::for_handle(&library).load_catalog().unwrap().unwrap()).unwrap(), cache_before);
            assert_eq!(Store::for_handle(&library).stamp().unwrap(), stamp_before, "nothing was written");
        }
        // A misspelt field is refused instead of silently ignored.
        assert!(serde_json::from_value::<MetadataEdit>(json!({"id":"a","tag":["x"]})).is_err());
        assert!(serde_json::from_value::<MetadataEdit>(json!({"id":"a","provenance":{"filesystemPath":"x"}})).is_err());
    }

    #[test]
    fn a_library_that_cannot_be_written_or_a_remote_one_is_refused() {
        let base = Base::new("refused");
        let library = seeded(&base.0);
        let path = library.root.join(config::LIBRARY_DB_REL);
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        let denied = update_entries(&library, &[edit("uid://night", Some(&["x"]))]);
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        std::fs::set_permissions(&path, permissions).unwrap();
        assert!(denied.unwrap_err().to_string().contains("read-only"));
        let mut remote = library.clone();
        remote.kind = crate::infra::registry::LibraryKind::Remote;
        assert!(update_entries(&remote, &[edit("uid://night", Some(&["x"]))]).is_err());
    }

    #[test]
    fn without_a_usable_cache_the_catalog_is_built_from_the_manifest() {
        let base = Base::new("nocache");
        let library = seeded(&base.0);
        Store::for_handle(&library).discard_catalog().unwrap();
        assert!(!Store::for_handle(&library).catalog_exists());
        let after = update_entries(&library, &[edit("uid://night", Some(&["sky"]))]).unwrap();
        assert_eq!(after.iter().find(|a| a.id == "uid://night").unwrap().tags, vec!["sky"]);
        assert!(Store::for_handle(&library).catalog_exists());
    }

    #[test]
    fn only_the_newest_backups_are_kept() {
        let base = Base::new("backups");
        let library = seeded(&base.0);
        for round in 0..(KEEP_BACKUPS + 4) {
            update_entries(&library, &[edit("uid://night", Some(&[&format!("round{round}")]))]).unwrap();
        }
        let kept = std::fs::read_dir(library.root.join("_library_config")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(BACKUP_PREFIX)).count();
        assert_eq!(kept, KEEP_BACKUPS);
    }

    fn give_personal_tags(library: &LibraryHandle, extra: &[(&str, &[&str])]) {
        let mut cached = catalog_of(library);
        for asset in &mut cached.assets {
            for (id, list) in extra {
                if asset.id == *id {
                    asset.user.tags.extend(list.iter().map(|t| t.to_string()));
                }
                for variant in &mut asset.variants {
                    if variant.id == *id {
                        variant.user.tags.extend(list.iter().map(|t| t.to_string()));
                    }
                }
            }
        }
        save_catalog_inner(library, &cached).unwrap();
    }

    fn backups(library: &LibraryHandle) -> usize {
        std::fs::read_dir(library.root.join("_library_config")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(BACKUP_PREFIX)).count()
    }

    #[test]
    fn personal_tags_move_into_the_library_once_and_provenance_tags_stay() {
        let base = Base::new("migrate");
        let library = seeded(&base.0);
        give_personal_tags(&library, &[("uid://crate", &["Mine", "WOOD", "odd: one"]), ("uid://crate_lod", &["lod-mine"]), ("uid://night", &["sky"])]);
        let read = catalog_of(&library);
        assert!(has_personal_tags(&read));

        let moved = migrate_personal_tags(&library, &read);
        assert_eq!(moved, 4, "mine, wood (already a library tag), lod-mine, sky");

        let crate_entry = entry(&library, "uid://crate");
        assert_eq!(crate_entry["tags"], json!(["wood", "box", "mine"]));
        assert_eq!(crate_entry["variants"][0]["tags"], json!(["lod-mine"]));
        assert_eq!(entry(&library, "uid://night")["tags"], json!(["sky"]));

        let now = catalog_of(&library);
        let item = now.assets.iter().find(|a| a.id == "uid://crate").unwrap();
        assert_eq!(item.tags, vec!["wood", "box", "mine"]);
        assert!(item.user.tags.contains(&"odd: one".to_string()), "a tag that cannot be a library tag stays");
        assert!(item.user.tags.contains(&"source: Crate Pack".to_string()) && item.user.tags.contains(&"license: CC0".to_string()));
        assert!(!item.user.tags.iter().any(|t| t == "Mine" || t == "WOOD"));
        assert!(item.variants[0].user.tags.iter().all(|t| t != "lod-mine"));
        assert!(now.assets.iter().find(|a| a.id == "uid://night").unwrap().user.tags.is_empty());

        // Idempotent and quiet: the leftover colon tag is not worth a write.
        let manifest_before = manifest(&library);
        let backups_before = backups(&library);
        assert_eq!(migrate_personal_tags(&library, &now), 0);
        assert_eq!(manifest(&library), manifest_before);
        assert_eq!(backups(&library), backups_before);
    }

    #[test]
    fn a_library_without_personal_tags_is_not_written_at_load() {
        let base = Base::new("migrate-none");
        let library = seeded(&base.0);
        let manifest_before = manifest(&library);
        let stamp_before = Store::for_handle(&library).stamp().unwrap();
        let cache_before = serde_json::to_value(Store::for_handle(&library).load_catalog().unwrap().unwrap()).unwrap();
        let loaded = load_catalog_migrating(&library).unwrap().unwrap();
        assert!(!has_personal_tags(&loaded));
        assert_eq!(manifest(&library), manifest_before);
        assert_eq!(serde_json::to_value(Store::for_handle(&library).load_catalog().unwrap().unwrap()).unwrap(), cache_before);
        assert_eq!(Store::for_handle(&library).stamp().unwrap(), stamp_before, "nothing was written");
        assert_eq!(backups(&library), 0);
    }

    #[test]
    fn loading_a_library_moves_its_personal_tags_and_a_full_item_keeps_the_rest() {
        let base = Base::new("migrate-load");
        let library = seeded(&base.0);
        let many: Vec<String> = (0..tags::MAX_TAGS).map(|n| format!("t{n}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        update_entries(&library, &[edit("uid://night", Some(&refs))]).unwrap();
        give_personal_tags(&library, &[("uid://night", &["overflow"]), ("uid://crate", &["mine"])]);

        let loaded = load_catalog_migrating(&library).unwrap().unwrap();
        let night = loaded.assets.iter().find(|a| a.id == "uid://night").unwrap();
        assert_eq!(night.tags.len(), tags::MAX_TAGS);
        assert!(night.user.tags.contains(&"overflow".to_string()), "no room left: it stays where it was, still shown");
        let crate_asset = loaded.assets.iter().find(|a| a.id == "uid://crate").unwrap();
        assert!(crate_asset.tags.contains(&"mine".to_string()) && !crate_asset.user.tags.contains(&"mine".to_string()));
    }

    #[test]
    fn personal_tags_of_a_library_that_cannot_be_written_are_left_in_place() {
        let base = Base::new("migrate-readonly");
        let library = seeded(&base.0);
        give_personal_tags(&library, &[("uid://crate", &["mine"])]);
        let path = library.root.join(config::LIBRARY_DB_REL);
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        let loaded = load_catalog_migrating(&library);
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        std::fs::set_permissions(&path, permissions).unwrap();
        let loaded = loaded.unwrap().unwrap();
        let item = loaded.assets.iter().find(|a| a.id == "uid://crate").unwrap();
        assert!(item.user.tags.contains(&"mine".to_string()));
        assert_eq!(item.tags, vec!["wood", "box"]);
        // A remote handle is never migrated here either.
        let mut remote = library.clone();
        remote.kind = crate::infra::registry::LibraryKind::Remote;
        assert_eq!(migrate_personal_tags(&remote, &loaded), 0);
    }

    #[test]
    fn edited_assets_and_remote_updates_use_the_same_patch() {
        let base = Base::new("remote");
        let library = seeded(&base.0);
        let edits = vec![edit("uid://crate_lod", Some(&["lod"]))];
        let picked = update_entries(&library, &edits).unwrap();
        assert_eq!(edited_assets(&catalog_of(&library), &edits).iter().map(|a| a.id.clone()).collect::<Vec<_>>(), picked.iter().map(|a| a.id.clone()).collect::<Vec<_>>());
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].id, "uid://crate", "an edited variant comes inside its parent");
        // A client holding an older copy of the catalog is brought up to date.
        let mut stale = catalog_of(&library);
        stale.assets.iter_mut().for_each(|a| { a.tags.clear(); a.variants.iter_mut().for_each(|v| v.tags.clear()); });
        let mut public = picked.clone();
        for asset in &mut public {
            asset.user.provenance = Some(ImportProvenance { source: Some("New".into()), ..Default::default() });
        }
        apply_updated_assets(&mut stale, &public);
        let crate_asset = stale.assets.iter().find(|a| a.id == "uid://crate").unwrap();
        assert_eq!(crate_asset.variants[0].tags, vec!["lod"]);
        assert_eq!(crate_asset.tags, picked[0].tags);
        assert!(crate_asset.user.tags.contains(&"source: New".to_string()));
        assert_eq!(crate_asset.user.provenance.as_ref().unwrap().filesystem_path.as_deref(), Some("P:/packs/crate"), "the importer's path stays on this computer");
    }
}
