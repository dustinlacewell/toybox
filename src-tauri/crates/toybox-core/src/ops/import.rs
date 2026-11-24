//! `merge_entries` (the `merge_seed_entries` command): the importer commit. An importer's entries have exactly
//! the shape of a manifest (`_library_config/catalog.json`) entry; the manifest is
//! the single source of truth, so the commit upserts them into it and the cached
//! catalog gains the new assets without losing any other.
//!
//! The cost of a commit depends on the entries it carries, never on the size of
//! the library: only the incoming entries are validated and derived
//! (`build_catalog`), only their files are placed, and only their rows are
//! written, in one transaction (`Store::apply`). What an incoming entry replaces
//! or absorbs is found through indexes (`items.pkey`, `item_variants`, `assets.rel_path`).
//!
//! Order matters: every entry is parsed and its files validated (via
//! `build_catalog`) before anything is written, so a bad import leaves the
//! library untouched. The referenced files must already exist under the library.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::Value;

use crate::domain::catalog_model::Asset;
use crate::domain::{merge, paths, seed};
use crate::error::{AppError, AppResult};
use crate::infra::registry::LibraryHandle;
use crate::infra::store::{self, Delta, ItemPut, Store};
use crate::infra::{animation_import, storage};

use super::catalog_build::{build_catalog, build_catalog_with};

/// What a commit wrote.
#[derive(Debug, Clone, Default)]
pub struct Merged {
    /// The top-level catalog assets the commit wrote, as they are now (personal
    /// data carried over from what they replaced, content hashes filled).
    pub assets: Vec<Asset>,
    /// Ids of the items that left the library because the commit replaced them
    /// (animation-only sources) or absorbed them (the same physical file).
    pub removed: Vec<String>,
}

pub fn merge_entries(handle: &LibraryHandle, entries: Vec<Value>) -> AppResult<Merged> {
    if handle.is_remote() {
        return Err(AppError::msg("Items are added to a remote library by uploading them to its server"));
    }
    if entries.is_empty() {
        return Ok(Merged::default());
    }
    let root = handle.root_str();
    // Finish the versioned storage migration before accepting any new writes; a
    // library without a current catalog gets one (a rescan) first.
    super::catalog::ready_for_write(handle)?;
    let parsed = entries.iter().map(seed::parse_entry).collect::<AppResult<Vec<_>>>()?;
    // Validate the incoming files before anything is written.
    build_catalog(&root, parsed)?;

    storage::exclusive(&handle.root, &handle.cache_path, |canonical| merge_locked(handle, canonical, entries))
}

fn str_id(entry: &Value) -> AppResult<String> {
    entry.get("id").and_then(Value::as_str).map(String::from).ok_or_else(|| AppError::msg("missing string field 'id'"))
}

/// The entries of the commit, after what they replace and absorb is settled.
struct Upserts {
    /// Ids in write order.
    order: Vec<String>,
    entries: HashMap<String, Value>,
    /// Items of the library an incoming entry absorbs (same physical file), by id.
    removed: BTreeMap<String, Value>,
    /// Entries as the library had them, for every item the commit replaces or removes.
    before: BTreeMap<String, Value>,
}

/// `seed::upsert_entries` against the database: an entry whose id exists replaces
/// it in place (keeping its provenance and, if it brings none, its tags); one
/// whose physical file another item has absorbs that item; the rest are new.
fn plan_upsert(store: &Store, incoming: Vec<Value>) -> AppResult<Upserts> {
    let ids: Vec<String> = incoming.iter().map(str_id).collect::<AppResult<_>>()?;
    let existing: HashMap<String, Value> = store.load_entries(&ids)?.into_iter().filter_map(|entry| Some((str_id(&entry).ok()?, entry))).collect();
    let mut upserts = Upserts { order: Vec::new(), entries: HashMap::new(), removed: BTreeMap::new(), before: BTreeMap::new() };
    // The library's items for every physical key of the commit, asked for once.
    let keys_of = |entry: &Value| -> Vec<String> {
        let mut keys: Vec<String> = seed::physical_key_text(entry).into_iter().collect();
        keys.extend(entry.get("variants").and_then(Value::as_array).into_iter().flatten().filter_map(seed::physical_key_text));
        keys
    };
    let mut all_keys: Vec<String> = incoming.iter().flat_map(|entry| keys_of(entry)).collect();
    all_keys.sort();
    all_keys.dedup();
    let mut holders: HashMap<String, Vec<(String, Value)>> = HashMap::new();
    for (holder, value) in store.load_entries_by_pkey(&all_keys)? {
        if let Some(key) = seed::physical_key_text(&value) {
            holders.entry(key).or_default().push((holder, value));
        }
    }
    for (mut entry, id) in incoming.into_iter().zip(ids) {
        let keys = keys_of(&entry);
        if !keys.is_empty() {
            // Earlier entries of this commit that this one absorbs.
            let absorbed: Vec<String> = upserts
                .entries
                .iter()
                .filter(|(other, value)| **other != id && seed::physical_key_text(value).is_some_and(|key| keys.contains(&key)))
                .map(|(other, _)| other.clone())
                .collect();
            for other in absorbed {
                upserts.entries.remove(&other);
                upserts.order.retain(|kept| *kept != other);
            }
            // Items of the library it absorbs.
            for (other, value) in keys.iter().filter_map(|key| holders.get(key)).flatten().cloned() {
                if other != id && !upserts.entries.contains_key(&other) && !upserts.removed.contains_key(&other) {
                    upserts.before.entry(other.clone()).or_insert_with(|| value.clone());
                    upserts.removed.insert(other, value);
                }
            }
        }
        let old = upserts.entries.get(&id).cloned().or_else(|| if upserts.removed.contains_key(&id) { None } else { existing.get(&id).cloned() });
        if let Some(old) = &old {
            seed::carry_from_old(old, &mut entry);
        }
        if let Some(stored) = existing.get(&id) {
            upserts.before.entry(id.clone()).or_insert_with(|| stored.clone());
        }
        if upserts.entries.insert(id.clone(), entry).is_none() {
            upserts.order.push(id);
        }
    }
    Ok(upserts)
}

fn merge_locked(handle: &LibraryHandle, canonical: &std::path::Path, incoming: Vec<Value>) -> AppResult<Merged> {
    let mut trace = crate::infra::trace::Trace::new("import");
    let root = handle.root_str();
    let store = Store::for_handle(handle);
    convert_archives_about_to_go(handle, canonical, &store, &incoming)?;
    let expect = store::manifest_revision(canonical)?;

    let mut upserts = plan_upsert(&store, incoming)?;
    trace.phase("lookup");
    let ordered: Vec<Value> = upserts.order.iter().map(|id| upserts.entries[id].clone()).collect();
    let prepared = animation_import::prepare_entries(&root, ordered)?;
    trace.phase("animation prepare");
    let mut entries = prepared.entries;
    // Clips cut from the incoming models: one replaces an item of its id, any other is new.
    let generated_ids: Vec<String> = prepared.generated.iter().filter_map(|entry| entry["id"].as_str().map(String::from)).collect();
    let stored_clips: HashMap<String, Value> = store.load_entries(&generated_ids)?.into_iter().filter_map(|entry| Some((str_id(&entry).ok()?, entry))).collect();
    for clip in prepared.generated {
        let id = str_id(&clip)?;
        if let Some(slot) = entries.iter_mut().find(|entry| entry["id"] == clip["id"]) {
            *slot = clip;
        } else {
            if let Some(stored) = stored_clips.get(&id) {
                upserts.before.entry(id).or_insert_with(|| stored.clone());
            }
            entries.push(clip);
        }
    }
    // An animation-only source is replaced by its clips: if the library holds one, it goes.
    let mut remove_items: Vec<String> = upserts.removed.keys().cloned().collect();
    if !prepared.replaced.is_empty() {
        for stored in store.load_entries(&prepared.replaced)? {
            let id = str_id(&stored)?;
            upserts.before.entry(id.clone()).or_insert(stored);
            if !remove_items.contains(&id) {
                remove_items.push(id);
            }
        }
    }

    let mut plan = storage::ItemPlan::new(canonical, entries)?;
    trace.phase("plan (hash sources)");
    plan.place()?;
    trace.phase("place files");
    let stored_entries: Vec<Value> = plan.entries().to_vec();
    let built = (|| -> AppResult<(Vec<Vec<Asset>>, Vec<crate::domain::rig::Rig>)> {
        let parsed = stored_entries.iter().map(seed::parse_entry).collect::<AppResult<Vec<_>>>()?;
        let (catalog, owners) = build_catalog_with(&root, parsed, &store)?;
        let mut groups: Vec<Vec<Asset>> = vec![Vec::new(); stored_entries.len()];
        for (asset, owner) in catalog.assets.into_iter().zip(owners) {
            groups[owner].push(asset);
        }
        Ok((groups, catalog.rigs))
    })();
    let (mut groups, rigs) = match built {
        Ok(built) => built,
        Err(error) => {
            plan.discard();
            return Err(error);
        }
    };

    trace.phase("derive");
    // Personal data of what the new assets replace or absorb, as a rescan would carry it.
    carry_user_data(&store, &mut groups)?;
    // Fingerprints of what is new (best effort: an unreadable item stays unhashed).
    let flat: Vec<Asset> = groups.iter().flatten().cloned().collect();
    let hashes = super::content_hash::missing_hashes(&handle.root, &handle.library_db_path(), &flat);

    trace.phase("user data + hashes");
    let puts: Vec<ItemPut> = stored_entries
        .iter()
        .zip(&groups)
        .map(|(entry, assets)| {
            let mut used = Vec::new();
            for asset in assets {
                collect_rig_ids(asset, &mut used);
            }
            ItemPut {
                id: entry["id"].as_str().unwrap_or("").to_string(),
                entry: Some(entry.clone()),
                assets: assets.clone(),
                rigs: rigs.iter().filter(|rig| used.contains(&rig.id)).cloned().collect(),
            }
        })
        .collect();
    let delta = Delta { puts, remove_items, hashes, expect_manifest_revision: Some(expect), ..Default::default() };
    let before = upserts.before;
    let removed: Vec<String> = delta.remove_items.clone();
    plan.commit(&handle.cache_path, before.clone(), delta)?;
    trace.phase("journal + rows + retire");

    // An item replaced in place now lives in a new folder. Its old one — maps
    // and any source archive — is no longer part of the library.
    let kept: BTreeSet<String> = folders_of(stored_entries.iter());
    for folder in folders_of(before.values()).into_iter().filter(|folder| !kept.contains(folder)) {
        if !store.folder_in_use(&folder).unwrap_or(true) {
            let _ = std::fs::remove_dir_all(paths::abs_under_root(&root, &folder));
        }
    }

    // Kept sources the replaced items listed and nothing lists any more leave with them.
    let replaced_sources: Vec<String> = shared_sources_of(before.values()).difference(&shared_sources_of(stored_entries.iter())).cloned().collect();
    if !replaced_sources.is_empty() {
        if let Ok(in_use) = store.files_in_use(&replaced_sources) {
            for path in replaced_sources.iter().filter(|path| !in_use.contains(*path)) {
                let file = paths::abs_under_root(&root, path);
                if std::fs::remove_file(&file).is_ok() {
                    crate::infra::retention::remove_set_folder_if_empty(&file);
                }
            }
        }
    }

    let mut assets: Vec<Asset> = groups.into_iter().flatten().collect();
    super::content_hash::fill_assets(handle, &mut assets);
    let written: HashSet<&str> = stored_entries.iter().filter_map(|entry| entry["id"].as_str()).collect();
    let removed = removed.into_iter().filter(|id| !written.contains(id.as_str())).collect();
    Ok(Merged { assets, removed })
}

/// Library migration step 6 turns each Substance archive a material keeps into a substance asset. A
/// replacement derived from such a substance (`origin.derivedFrom`, the new plugin's shape) that arrives
/// while the old material still lists the archive and the substance does not exist yet (the conversion
/// was skipped, or an older client committed the old material through this host) would leave the archive
/// listed by nothing, and the commit would remove it: the only copy. Convert it first.
fn convert_archives_about_to_go(handle: &LibraryHandle, canonical: &std::path::Path, store: &Store, incoming: &[Value]) -> AppResult<()> {
    use crate::infra::substance_migration::{sbsar_source, substance_id, convert_before_replacement};
    let derived: BTreeSet<String> = incoming
        .iter()
        .flat_map(|entry| std::iter::once(entry).chain(entry["variants"].as_array().into_iter().flatten()))
        .filter_map(|item| item["origin"]["derivedFrom"].as_str().filter(|id| id.starts_with("uid://substance_")).map(String::from))
        .collect();
    if derived.is_empty() {
        return Ok(());
    }
    // From the entries, not the derived rows (which may be stale).
    let present: HashSet<String> = store.load_entries(&derived.iter().cloned().collect::<Vec<_>>())?.iter().filter_map(|entry| entry["id"].as_str().map(String::from)).collect();
    if derived.iter().all(|id| present.contains(id)) {
        return Ok(());
    }
    // What the commit replaces (read only: planned again after the conversion).
    let upserts = plan_upsert(store, incoming.to_vec())?;
    let kept = shared_sources_of(upserts.entries.values());
    for archive in shared_sources_of(upserts.before.values()).difference(&kept) {
        let Some((sha, _)) = sbsar_source(archive) else { continue };
        let substance = substance_id(sha);
        if !derived.contains(&substance) || present.contains(&substance) || !paths::abs_under_root(&handle.root_str(), archive).is_file() {
            continue;
        }
        let ids: Vec<String> = upserts.before.iter().filter(|(_, entry)| shared_sources_of(std::iter::once(*entry)).contains(archive)).map(|(id, _)| id.clone()).collect();
        convert_before_replacement(canonical, &handle.cache_path, store, archive, &ids)?;
    }
    Ok(())
}

fn collect_rig_ids(asset: &Asset, out: &mut Vec<String>) {
    out.extend(asset.rigs.iter().map(|binding| binding.rig_id.clone()));
    for variant in &asset.variants {
        collect_rig_ids(variant, out);
    }
}

/// `merge_preserving_user` for the assets of one commit: what each new asset
/// replaces or absorbs (found by id and by path) hands over its favourite,
/// personal tags, collections, preview choice and thumbnail.
fn carry_user_data(store: &Store, groups: &mut [Vec<Asset>]) -> AppResult<()> {
    let mut ids = Vec::new();
    let mut rel_paths = Vec::new();
    for asset in groups.iter().flatten() {
        let (more_ids, more_paths) = merge::old_member_keys(asset);
        ids.extend(more_ids);
        rel_paths.extend(more_paths);
    }
    ids.sort();
    ids.dedup();
    rel_paths.sort();
    rel_paths.dedup();
    let stored = store.load_assets_matching(&ids, &rel_paths)?;
    let mut done: Vec<Asset> = Vec::new();
    for group in groups.iter_mut() {
        for asset in group.iter_mut() {
            let members: Vec<&Asset> = stored.iter().chain(done.iter()).filter(|old| merge::is_old_member(asset, old)).collect();
            merge::absorb(asset, &members);
            done.push(asset.clone());
        }
    }
    Ok(())
}

/// The shared source paths (`origin.files`) these entries and their variants list.
fn shared_sources_of<'a>(entries: impl Iterator<Item = &'a Value>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for entry in entries {
        for item in std::iter::once(entry).chain(entry["variants"].as_array().into_iter().flatten()) {
            for file in item["origin"]["files"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if crate::domain::sources::is_shared(file) {
                    out.insert(file.to_string());
                }
            }
        }
    }
    out
}

/// Managed folders of folder-stored items (materials and plugin-defined types)
/// used by these entries and their variants.
fn folders_of<'a>(entries: impl Iterator<Item = &'a Value>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for entry in entries {
        for item in std::iter::once(entry).chain(entry["variants"].as_array().into_iter().flatten()) {
            if let Some(path) = managed_folder(item) {
                out.insert(path);
            }
        }
    }
    out
}

/// The `library/<type>/<key>` folder of a material or plugin item, if it is well formed.
fn managed_folder(item: &Value) -> Option<String> {
    if !matches!(item["kind"].as_str(), Some("material" | "plugin")) {
        return None;
    }
    let path = item["storagePath"].as_str()?;
    let parts: Vec<&str> = path.split('/').collect();
    let key = parts.get(2).copied().unwrap_or("");
    let well_formed = parts.len() == 3
        && parts[0] == "library"
        && !parts[1].is_empty()
        && !parts[1].starts_with('.')
        && matches!(key.len(), 16 | 64)
        && key.bytes().all(|b| b.is_ascii_hexdigit())
        && !path.contains(['\\', ':']);
    well_formed.then(|| path.to_string())
}

/// Managed folders of folder-stored items that `before` used and `after` no longer does.
#[cfg(test)]
fn retired_folders(before: &Value, after: &Value) -> Vec<String> {
    let entries = |doc: &Value| -> BTreeSet<String> { folders_of(doc["assets"].as_array().into_iter().flatten()) };
    let kept = entries(after);
    entries(before).into_iter().filter(|folder| !kept.contains(folder)).collect()
}

#[cfg(test)]
mod retire_tests {
    use super::retired_folders;
    use serde_json::json;

    #[test]
    fn only_folders_a_replaced_item_left_behind_are_retired() {
        let a = format!("library/materials/{}", "a".repeat(64));
        let b = format!("library/materials/{}", "b".repeat(64));
        let c = format!("library/materials/{}", "c".repeat(64));
        let sky = format!("library/skybox/{}", "d".repeat(64));
        let before = json!({"assets":[
            {"id":"tiles","kind":"material","storagePath":a,"variants":[{"id":"tiles_1k","kind":"material","storagePath":b}]},
            {"id":"other","kind":"material","storagePath":c},
            {"id":"sky","kind":"plugin","type":"skybox","storagePath":sky},
            {"id":"model","kind":"model","storagePath":format!("library/models/{}/m.gltf", "e".repeat(64))},
            {"id":"odd","kind":"material","storagePath":"library/materials/../../elsewhere"},
        ]});
        // `tiles` re-rendered into a new folder; its variant and the rest stay.
        let fresh = format!("library/materials/{}", "f".repeat(64));
        let mut after = before.clone();
        after["assets"][0]["storagePath"] = json!(fresh);
        assert_eq!(retired_folders(&before, &after), vec![a.clone()]);
        // Nothing changed, nothing retired; a folder two items share is kept.
        assert!(retired_folders(&before, &before).is_empty());
        after["assets"][1]["storagePath"] = json!(a);
        assert_eq!(retired_folders(&before, &after), vec![c.clone()]);
        // Removed items are the delete command's business, but their folders
        // are equally unreferenced; malformed paths are never named.
        let emptied = json!({"assets":[]});
        assert_eq!(retired_folders(&before, &emptied), vec![a, b, c, sky]);
    }
}
