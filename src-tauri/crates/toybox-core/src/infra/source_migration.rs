//! Library migration step 5, `shared-sources`: move every kept source file (`origin.files`) of the
//! library into shared, content-addressed storage (`library/<type>/_sources/<sha256>/<name>`, see
//! `domain::sources`), one copy per distinct content.
//!
//! The work is done in batches through the same journaled publication an import uses
//! ([`storage::ItemPlan`]): for each batch the shared copies are placed and verified (size and hash),
//! then one transaction rewrites the entries and the derived rows, and only then are the old copies
//! beside the items removed (each only if it still is what was hashed). Every point in between is
//! valid: before the transaction the entries still name the old files, after it they name the shared
//! ones; a crash is finished by the journal. A source that cannot be moved (missing, unreadable) stays
//! where it is, which still reads correctly, and is reported.
//!
//! Progress is recorded in the library database (`meta`): the step is done when `sharedSources` is
//! `done`. The library journal is advanced to version 5 *before* the first file moves, so an older
//! Toybox refuses the library from then on, and a run that was interrupted resumes at the next open.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::sources;
use crate::error::AppResult;
use crate::infra::storage::{self, ItemPlan};
use crate::infra::store::{Delta, ItemPut, Store};

/// Items per publication.
const BATCH: usize = 40;
pub const META_DONE: &str = "sharedSources";
pub const META_REPORT: &str = "sharedSourcesReport";

/// What the migration did.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceMigration {
    /// Items whose entry now names shared sources.
    pub items: usize,
    /// Distinct source files now in shared storage.
    pub shared_files: usize,
    /// Bytes the items' own copies took before.
    pub bytes_before: u64,
    /// Bytes freed: the old copies minus the shared ones.
    pub bytes_freed: u64,
    /// Items that could not be converted and why; they keep their sources beside them.
    pub skipped: Vec<(String, String)>,
}

/// Whether the library's sources are in shared storage already.
pub fn is_done(store: &Store) -> bool {
    store.meta(META_DONE).as_deref() == Some("done")
}

/// The report of the finished migration, if one ran.
pub fn report(store: &Store) -> Option<SourceMigration> {
    store.meta(META_REPORT).and_then(|text| serde_json::from_str(&text).ok())
}

/// Whether an entry (or one of its variants) lists a source file that is not in shared storage.
pub fn has_unshared_sources(entry: &Value) -> bool {
    std::iter::once(entry).chain(entry["variants"].as_array().into_iter().flatten()).any(|item| {
        item["origin"]["files"].as_array().into_iter().flatten().filter_map(Value::as_str).any(|file| !sources::is_shared(file))
    })
}

fn size_of(root: &Path, relative: &str) -> u64 {
    std::fs::metadata(crate::domain::paths::abs_under_root(&root.to_string_lossy(), relative)).map(|meta| meta.len()).unwrap_or(0)
}

/// Convert the library at `root` (cache path `cache`). Resumable and idempotent.
pub fn migrate(root: &Path, cache: &Path) -> AppResult<SourceMigration> {
    let store = Store::local(root, cache);
    let mut total = report(&store).unwrap_or_default();
    storage::exclusive(root, cache, |canonical| {
        let mut after = String::new();
        let mut counted: HashSet<String> = HashSet::new();
        loop {
            let (entries, next) = store.entries_page_with_origin(&after, BATCH)?;
            let pending: Vec<Value> = entries.into_iter().filter(has_unshared_sources).collect();
            if !pending.is_empty() {
                let done = match batch(canonical, cache, &store, &pending, &mut counted) {
                    Ok(done) => done,
                    Err(_) => {
                        // One bad item must not hold back its neighbours: the batch again, one by one.
                        let mut done = SourceMigration::default();
                        for entry in &pending {
                            let id = entry["id"].as_str().unwrap_or("").to_string();
                            match batch(canonical, cache, &store, std::slice::from_ref(entry), &mut counted) {
                                Ok(part) => merge(&mut done, part),
                                Err(error) => done.skipped.push((id, error.to_string())),
                            }
                        }
                        done
                    }
                };
                merge(&mut total, done);
            }
            match next {
                Some(next) => after = next,
                None => break,
            }
        }
        Ok(())
    })?;
    // Skipped items from an earlier run that have been converted since are no longer a problem.
    total.skipped.retain(|(id, _)| store.load_entries(std::slice::from_ref(id)).ok().and_then(|entries| entries.into_iter().next()).is_some_and(|entry| has_unshared_sources(&entry)));
    store.set_meta(META_REPORT, &serde_json::to_string(&total)?)?;
    store.set_meta(META_DONE, "done")?;
    Ok(total)
}

fn merge(into: &mut SourceMigration, part: SourceMigration) {
    into.items += part.items;
    into.shared_files += part.shared_files;
    into.bytes_before += part.bytes_before;
    into.bytes_freed += part.bytes_freed;
    into.skipped.extend(part.skipped);
}

/// Publish one batch of entries.
fn batch(canonical: &Path, cache: &Path, store: &Store, entries: &[Value], counted: &mut HashSet<String>) -> AppResult<SourceMigration> {
    let before: BTreeMap<String, Value> = entries.iter().filter_map(|entry| Some((entry["id"].as_str()?.to_string(), entry.clone()))).collect();
    let mut plan = ItemPlan::new(canonical, entries.to_vec())?;
    // Sizes of the old copies, and of each shared file that is new, before anything moves.
    let moved = plan.moved_sources();
    let old: u64 = moved.keys().map(|path| size_of(canonical, path)).sum();
    let mut created: u64 = 0;
    let mut new_files = 0;
    for target in moved.values().collect::<HashSet<_>>() {
        if counted.insert(target.clone()) {
            new_files += 1;
            if !crate::domain::paths::abs_under_root(&canonical.to_string_lossy(), target).is_file() {
                created += size_of(canonical, moved.iter().find(|(_, t)| *t == target).map(|(s, _)| s.as_str()).unwrap_or(""));
            }
        }
    }
    plan.place()?;
    let ids: Vec<String> = plan.entries().iter().filter_map(|entry| entry["id"].as_str().map(String::from)).collect();
    let mut assets = store.load_top_assets(&ids)?;
    let mut puts = Vec::new();
    for entry in plan.entries() {
        let id = entry["id"].as_str().unwrap_or("").to_string();
        let mut mine: Vec<_> = assets.iter().filter(|asset| asset.id == id).cloned().collect();
        for asset in &mut mine {
            relink(asset, &moved);
        }
        puts.push(ItemPut { id, entry: Some(entry.clone()), assets: mine, rigs: Vec::new() });
    }
    assets.clear();
    let expect = crate::infra::store::manifest_revision(canonical)?;
    let items = puts.len();
    plan.commit(cache, before, Delta { puts, expect_manifest_revision: Some(expect), ..Default::default() })?;
    Ok(SourceMigration { items, shared_files: new_files, bytes_before: old, bytes_freed: old.saturating_sub(created), skipped: Vec::new() })
}

/// Point a derived asset (and its variants) at the shared copies.
fn relink(asset: &mut crate::domain::catalog_model::Asset, moved: &std::collections::HashMap<String, String>) {
    if let Some(origin) = asset.origin.as_mut() {
        for file in &mut origin.files {
            if let Some(target) = moved.get(file.as_str()) {
                *file = target.clone();
            }
        }
    }
    for variant in &mut asset.variants {
        relink(variant, moved);
    }
}
