//! Kept source files are stored once per library, by content: importing, replacing, deleting,
//! transferring, and converting a library that keeps a copy beside every item. (The source here is a
//! generic one: a Substance archive becomes an asset of its own in step 6, see `substance_tests`.)

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{json, Value};

use super::delete::delete_from_library;
use super::import::merge_entries;
use super::transfer::{export_package, ingest_package, local_reader, ExportOptions, IngestOptions};
use crate::domain::catalog_model::Asset;
use crate::domain::sources;
use crate::infra::fsio;
use crate::infra::registry::{LibraryEntry, LibraryHandle};
use crate::infra::source_migration;
use crate::infra::store::{Delta, ItemPut, Store};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Base(PathBuf);

impl Base {
    fn new(name: &str) -> Base {
        let dir = std::env::temp_dir().join(format!("toybox-sources-{name}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        Base(dir)
    }
}

impl Drop for Base {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn library(base: &Base, id: &str) -> LibraryHandle {
    let root = base.0.join(format!("root-{id}"));
    crate::infra::library::ensure_library_folder(&root).unwrap();
    let root = root.canonicalize().unwrap();
    LibraryHandle::local(&base.0.join("appdata"), &LibraryEntry::local(id, id, &root.to_string_lossy()))
}

fn put(library: &LibraryHandle, rel: &str, bytes: &[u8]) {
    fsio::write_bytes(&library.root.join(rel), bytes).unwrap();
}

fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// A material rendered from `archive` (staged as an importer does: its maps and a copy of the archive).
fn preset(library: &LibraryHandle, name: &str, archive: &[u8]) -> Value {
    put(library, &format!("library/_import_s/materials/{name}/color.png"), name.as_bytes());
    put(library, &format!("library/_import_s/materials/{name}/stone.src"), archive);
    json!({"id": format!("uid://{name}"), "kind": "material", "pack": "_import_s", "category": "stone", "name": name,
        "maps": [{"role": "albedo", "file": "color.png"}],
        "origin": {"plugin": "com.example.sbsar", "files": ["stone.src"], "meta": {"source": "stone.src", "preset": name}}})
}

fn assets(library: &LibraryHandle) -> Vec<Asset> {
    let store = Store::for_handle(library);
    let mut ids = Vec::new();
    store.for_each_asset_id(&mut |id| ids.push(id.to_string())).unwrap();
    store.load_top_assets(&ids).unwrap()
}

fn files_below(folder: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(files_below(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}

fn shared_files(library: &LibraryHandle) -> Vec<PathBuf> {
    files_below(&library.root.join("library/materials/_sources"))
}

#[test]
fn presets_of_one_archive_share_one_stored_copy_and_keep_their_own_keys() {
    let base = Base::new("presets");
    let lib = library(&base, "a");
    let archive = vec![7u8; 4096];
    let entries: Vec<Value> = ["Green", "Red", "Blue"].iter().map(|name| preset(&lib, name, &archive)).collect();
    merge_entries(&lib, entries).unwrap();

    let expected = sources::shared_path("materials", &hash(&archive), "stone.src");
    let items = assets(&lib);
    assert_eq!(items.len(), 3);
    let mut folders = Vec::new();
    for item in &items {
        assert_eq!(item.origin.as_ref().unwrap().files, vec![expected.clone()], "{} points at the shared copy", item.name);
        assert_eq!(item.origin.as_ref().unwrap().meta["source"], "stone.src");
        folders.push(item.rel_path.clone());
        assert!(!lib.root.join(&item.rel_path).join("stone.src").exists(), "no copy beside the maps");
    }
    folders.sort();
    folders.dedup();
    assert_eq!(folders.len(), 3, "the presets are distinct items with distinct folders");
    assert_eq!(shared_files(&lib), vec![lib.root.join(&expected)]);
    assert_eq!(std::fs::read(lib.root.join(&expected)).unwrap(), archive);
    assert!(!lib.root.join("library/_import_s").exists(), "the staging is gone, duplicates of the archive included");
    // The manifest says the same as the catalog.
    let entry = Store::for_handle(&lib).load_entries(&["uid://Green".to_string()]).unwrap().remove(0);
    assert_eq!(entry["origin"]["files"], json!([expected]));
    // The same archive imported again later adds nothing.
    merge_entries(&lib, vec![preset(&lib, "Yellow", &archive)]).unwrap();
    assert_eq!(shared_files(&lib).len(), 1);
    assert!(!lib.root.join("library/_import_s").exists());
}

#[test]
fn a_shared_source_goes_with_its_last_item_and_not_before() {
    let base = Base::new("delete");
    let lib = library(&base, "a");
    let archive = vec![3u8; 1024];
    let other = vec![4u8; 1024];
    merge_entries(&lib, vec![preset(&lib, "One", &archive), preset(&lib, "Two", &archive), preset(&lib, "Three", &other)]).unwrap();
    assert_eq!(shared_files(&lib).len(), 2);
    delete_from_library(&lib.root, &lib.cache_path, &["uid://One".to_string()]).unwrap();
    assert_eq!(shared_files(&lib).len(), 2, "Two still keeps the archive");
    delete_from_library(&lib.root, &lib.cache_path, &["uid://Two".to_string()]).unwrap();
    let left = shared_files(&lib);
    assert_eq!(left.len(), 1, "{left:?}");
    assert_eq!(std::fs::read(&left[0]).unwrap(), other);
    assert!(!lib.root.join(format!("library/materials/_sources/{}", hash(&archive))).exists(), "the emptied folder goes too");
    delete_from_library(&lib.root, &lib.cache_path, &["uid://Three".to_string()]).unwrap();
    assert!(shared_files(&lib).is_empty());
}

#[test]
fn replacing_an_item_with_another_archive_drops_the_old_one_when_nothing_else_keeps_it() {
    let base = Base::new("replace");
    let lib = library(&base, "a");
    let (old, new) = (vec![1u8; 512], vec![2u8; 512]);
    merge_entries(&lib, vec![preset(&lib, "Only", &old), preset(&lib, "Pair", &old)]).unwrap();
    merge_entries(&lib, vec![preset(&lib, "Only", &new)]).unwrap();
    let kept: Vec<Vec<u8>> = shared_files(&lib).iter().map(|path| std::fs::read(path).unwrap()).collect();
    assert_eq!(kept.len(), 2, "the old archive stays for Pair, the new one arrived");
    merge_entries(&lib, vec![preset(&lib, "Pair", &new)]).unwrap();
    let kept: Vec<Vec<u8>> = shared_files(&lib).iter().map(|path| std::fs::read(path).unwrap()).collect();
    assert_eq!(kept, vec![new], "nothing lists the old archive now, so it is gone");
}

#[test]
fn a_copy_between_libraries_carries_each_source_once_and_relinks_it() {
    let base = Base::new("transfer");
    let (from, to) = (library(&base, "from"), library(&base, "to"));
    let archive = vec![9u8; 2048];
    merge_entries(&from, vec![preset(&from, "A", &archive), preset(&from, "B", &archive), preset(&from, "C", &archive)]).unwrap();
    let ids: Vec<String> = ["A", "B", "C"].iter().map(|n| format!("uid://{n}")).collect();
    let package = export_package(&from, &ids, ExportOptions::default()).unwrap();
    let reads = std::sync::Mutex::new(Vec::<String>::new());
    let read = |path: &str| -> crate::error::AppResult<Box<dyn std::io::Read>> {
        reads.lock().unwrap().push(path.to_string());
        local_reader(&from)(path)
    };
    let report = ingest_package(&to, &package, IngestOptions::default(), &read, &|_| {}).unwrap();
    assert_eq!(report.items.len(), 3);
    assert!(report.items.iter().all(|item| item.reason.is_none()), "{report:?}");
    let archive_path = sources::shared_path("materials", &hash(&archive), "stone.src");
    assert_eq!(reads.lock().unwrap().iter().filter(|path| **path == archive_path).count(), 1, "the archive is read from the source once");
    assert_eq!(shared_files(&to), vec![to.root.join(&archive_path)]);
    for item in assets(&to) {
        assert_eq!(item.origin.unwrap().files, vec![archive_path.clone()]);
    }
    // Copying one more item when the target holds the archive reads nothing of it.
    merge_entries(&from, vec![preset(&from, "D", &archive)]).unwrap();
    reads.lock().unwrap().clear();
    let package = export_package(&from, &["uid://D".to_string()], ExportOptions::default()).unwrap();
    ingest_package(&to, &package, IngestOptions::default(), &read, &|_| {}).unwrap();
    assert!(!reads.lock().unwrap().iter().any(|path| *path == archive_path), "{:?}", reads.lock().unwrap());
    assert_eq!(shared_files(&to).len(), 1);
}

/// Put a library back into the layout of the version before: a copy of each source beside its item.
fn make_legacy(library: &LibraryHandle) {
    let store = Store::for_handle(library);
    let mut ids = Vec::new();
    store.for_each_asset_id(&mut |id| ids.push(id.to_string())).unwrap();
    for id in ids {
        let mut entry = store.load_entries(&[id.clone()]).unwrap().remove(0);
        let folder = entry["storagePath"].as_str().unwrap().to_string();
        let mut asset = store.load_top_assets(&[id.clone()]).unwrap().remove(0);
        let mut bare = Vec::new();
        let mut legacy = Vec::new();
        for shared in entry["origin"]["files"].as_array().unwrap().iter().filter_map(Value::as_str) {
            let name = sources::file_name(shared).to_string();
            std::fs::copy(library.root.join(shared), library.root.join(&folder).join(&name)).unwrap();
            legacy.push(format!("{folder}/{name}"));
            bare.push(name);
        }
        entry["origin"]["files"] = json!(bare);
        asset.origin.as_mut().unwrap().files = legacy;
        store.apply(&Delta { puts: vec![ItemPut { id: id.clone(), entry: Some(entry), assets: vec![asset], rigs: vec![] }], ..Default::default() }).unwrap();
    }
    std::fs::remove_dir_all(library.root.join("library/materials/_sources")).unwrap();
    store.set_meta(source_migration::META_DONE, "").unwrap();
}

#[test]
fn a_library_with_a_copy_beside_every_item_is_converted_in_place() {
    let base = Base::new("migrate");
    let lib = library(&base, "a");
    let archive = vec![5u8; 100_000];
    let lone = vec![6u8; 3000];
    merge_entries(&lib, vec![preset(&lib, "One", &archive), preset(&lib, "Two", &archive), preset(&lib, "Three", &archive), preset(&lib, "Solo", &lone)]).unwrap();
    make_legacy(&lib);
    assert_eq!(files_below(&lib.root.join("library/materials")).iter().filter(|p| p.file_name().unwrap() == "stone.src").count(), 4);

    crate::infra::migrations::run(&lib.root, &lib.cache_path).unwrap();

    let store = Store::for_handle(&lib);
    assert!(source_migration::is_done(&store));
    let report = source_migration::report(&store).unwrap();
    assert_eq!(report.items, 4);
    assert_eq!(report.shared_files, 2);
    assert_eq!(report.bytes_before, 3 * 100_000 + 3000);
    assert_eq!(report.bytes_freed, 2 * 100_000, "three copies became one");
    assert!(report.skipped.is_empty());
    assert_eq!(shared_files(&lib).len(), 2);
    let kept_beside = files_below(&lib.root.join("library/materials")).into_iter().filter(|p| p.file_name().unwrap() == "stone.src" && !p.to_string_lossy().contains("_sources")).count();
    assert_eq!(kept_beside, 0, "no copy is left beside an item");
    let expected = sources::shared_path("materials", &hash(&archive), "stone.src");
    for item in assets(&lib).iter().filter(|item| item.name != "Solo") {
        assert_eq!(item.origin.as_ref().unwrap().files, vec![expected.clone()]);
        assert_eq!(item.origin.as_ref().unwrap().meta["source"], "stone.src");
    }
    let entry = store.load_entries(&["uid://Two".to_string()]).unwrap().remove(0);
    assert_eq!(entry["origin"]["files"], json!([expected]));
    // The journal says the library needs a Toybox that knows the layout.
    let journal: Value = serde_json::from_slice(&std::fs::read(lib.root.join("_library_config/migrations.json")).unwrap()).unwrap();
    assert_eq!(journal["version"], crate::domain::library_migrations::LATEST_VERSION);
    // Running it again changes nothing.
    crate::infra::migrations::run(&lib.root, &lib.cache_path).unwrap();
    assert_eq!(source_migration::report(&store).unwrap(), report);
    // A rescan from the manifest finds everything where the entries say.
    let rescanned = super::scan::scan_library_catalog(&lib).unwrap();
    assert_eq!(rescanned.assets.len(), 4);
}

#[test]
fn an_interrupted_conversion_is_valid_at_every_point_and_finishes_later() {
    let base = Base::new("interrupted");
    let lib = library(&base, "a");
    let archive = vec![8u8; 20_000];
    merge_entries(&lib, vec![preset(&lib, "One", &archive), preset(&lib, "Two", &archive)]).unwrap();
    make_legacy(&lib);
    let store = Store::for_handle(&lib);
    // Killed after the shared copies were placed but before anything was published: the entries still
    // name the files beside the items, which are all still there.
    let canonical = lib.root.canonicalize().unwrap();
    crate::infra::storage::exclusive(&lib.root, &lib.cache_path, |_| {
        let (entries, _) = store.entries_page_with_origin("", 10)?;
        let mut plan = crate::infra::storage::ItemPlan::new(&canonical, entries)?;
        plan.place()?;
        Ok(())
    })
    .unwrap();
    assert_eq!(shared_files(&lib).len(), 1, "the shared copy is there");
    for item in assets(&lib) {
        assert!(lib.root.join(&item.origin.as_ref().unwrap().files[0]).is_file(), "{} still reads its own copy", item.name);
    }
    // The next open finishes the job and the duplicates go.
    crate::infra::migrations::run(&lib.root, &lib.cache_path).unwrap();
    assert!(source_migration::is_done(&store));
    assert_eq!(shared_files(&lib).len(), 1);
    for item in assets(&lib) {
        assert!(sources::is_shared(&item.origin.as_ref().unwrap().files[0]));
        assert!(lib.root.join(&item.origin.as_ref().unwrap().files[0]).is_file());
    }
}

#[test]
fn a_source_that_cannot_be_moved_stays_where_it_is_and_is_reported() {
    let base = Base::new("missing");
    let lib = library(&base, "a");
    merge_entries(&lib, vec![preset(&lib, "Fine", &[1u8; 100]), preset(&lib, "Broken", &[2u8; 100])]).unwrap();
    make_legacy(&lib);
    let store = Store::for_handle(&lib);
    let broken = store.load_entries(&["uid://Broken".to_string()]).unwrap().remove(0);
    std::fs::remove_file(lib.root.join(broken["storagePath"].as_str().unwrap()).join("stone.src")).unwrap();
    crate::infra::migrations::run(&lib.root, &lib.cache_path).unwrap();
    let report = source_migration::report(&store).unwrap();
    assert_eq!(report.items, 1);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].0, "uid://Broken");
    let fine = assets(&lib).into_iter().find(|item| item.name == "Fine").unwrap();
    assert!(sources::is_shared(&fine.origin.unwrap().files[0]));
    let broken = store.load_entries(&["uid://Broken".to_string()]).unwrap().remove(0);
    assert_eq!(broken["origin"]["files"], json!(["stone.src"]), "left as it was");
}

/// Every file below `root` with a hash of its content, in path order.
fn tree_hash(root: &std::path::Path) -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = files_below(root)
        .into_iter()
        .map(|path| (path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), hash(&std::fs::read(&path).unwrap())))
        .collect();
    all.sort();
    all
}

/// Set the library journal back to `version` (as a library that has not seen the later steps).
fn journal_at(library: &LibraryHandle, version: u32) {
    let path = library.root.join("_library_config/migrations.json");
    let mut journal: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    journal["version"] = json!(version);
    journal["applied"].as_array_mut().unwrap().retain(|step| step["version"].as_u64().unwrap() <= version as u64);
    std::fs::write(path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();
}

fn legacy_library(base: &Base) -> LibraryHandle {
    let lib = library(base, "a");
    let archive = vec![5u8; 100_000];
    merge_entries(&lib, vec![preset(&lib, "One", &archive), preset(&lib, "Two", &archive), preset(&lib, "Solo", &[6u8; 3000])]).unwrap();
    make_legacy(&lib);
    journal_at(&lib, 4);
    lib
}

#[test]
fn the_pre_flight_reports_step_five_and_changes_nothing() {
    let base = Base::new("preflight");
    let lib = legacy_library(&base);
    let before = tree_hash(&lib.root);
    let report = crate::infra::preflight::check(&lib.root).unwrap();
    assert_eq!(tree_hash(&lib.root), before, "the library is byte-identical after the check");
    assert_eq!(report.format_version, 4);
    assert_eq!(report.latest_version, 6);
    assert_eq!(report.pending.len(), 2, "step 5, and step 6 with nothing to convert in this library");
    assert_eq!((report.pending[1].version, report.pending[1].items), (6, Some(0)));
    let step = &report.pending[0];
    assert_eq!((step.version, step.name.as_str()), (5, "shared-sources"));
    assert_eq!(step.items, Some(3));
    assert_eq!(step.files, Some(3));
    assert_eq!(step.bytes, Some(2 * 100_000 + 3000));
    assert!(step.rewrites_entries);
    assert!(report.estimate_seconds.is_some());
    assert!(report.free_bytes.is_some() && report.needed_bytes > 0 && report.enough_space);
    let target = report.snapshot.clone().expect("it says where the snapshot will go");
    assert!(target.replace('\\', "/").contains("_library_config/snapshots/library-v4-"), "{target}");
    assert!(report.refused_before.is_some());
    assert!(!lib.root.join("_library_config/snapshots").exists(), "nothing was created");
    // Said twice, same answer, still nothing written.
    assert_eq!(crate::infra::preflight::check(&lib.root).unwrap().pending, report.pending);
    assert_eq!(tree_hash(&lib.root), before);
}

#[test]
fn the_conversion_takes_a_snapshot_first_and_keeps_the_newest_three() {
    let base = Base::new("snapshot");
    let lib = legacy_library(&base);
    crate::infra::migrations::run(&lib.root, &lib.cache_path).unwrap();
    let made = crate::infra::snapshot::list(&lib.root);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].from_version, 4);
    // It is a database of the library as it was: the entries name the files beside the items.
    let connection = crate::infra::snapshot::open_unchanged(&made[0].path).unwrap();
    let entry: String = connection.query_row("SELECT entry FROM items WHERE id = 'uid://One'", [], |row| row.get(0)).unwrap();
    assert!(entry.contains("\"stone.src\"") && !entry.contains("_sources"), "{entry}");
    drop(connection);
    // Nothing more to convert: no second snapshot on the next open.
    crate::infra::migrations::run(&lib.root, &lib.cache_path).unwrap();
    assert_eq!(crate::infra::snapshot::list(&lib.root).len(), 1);
    // Only the newest three are kept.
    for _ in 0..4 {
        crate::infra::snapshot::take(&lib.root, 5).unwrap();
    }
    let kept = crate::infra::snapshot::list(&lib.root);
    assert_eq!(kept.len(), crate::infra::snapshot::KEEP);
    assert!(kept.iter().all(|snapshot| snapshot.from_version == 5), "the oldest, the first one, was dropped");
    assert_eq!(std::fs::read_dir(lib.root.join("_library_config/snapshots")).unwrap().count(), 3, "no partial files left");
}

#[test]
fn a_snapshot_restores_entries_but_not_files_a_migration_moved() {
    let base = Base::new("restore");
    let lib = legacy_library(&base);
    crate::infra::migrations::run(&lib.root, &lib.cache_path).unwrap();
    let snapshot = crate::infra::snapshot::list(&lib.root).remove(0);
    let live_before = tree_hash(&lib.root);
    // The copies beside the items are gone: refused, and nothing changed.
    let error = crate::infra::snapshot::restore(&lib.root, &snapshot.name).unwrap_err().to_string();
    assert!(error.contains("no longer there"), "{error}");
    assert_eq!(tree_hash(&lib.root), live_before);

    // A snapshot taken while the files are all in place restores, and the journal goes back with it.
    let base2 = Base::new("restore-ok");
    let lib2 = legacy_library(&base2);
    let taken = crate::infra::snapshot::take(&lib2.root, 4).unwrap();
    let name = taken.file_name().unwrap().to_string_lossy().to_string();
    journal_at(&lib2, 4);
    let store = Store::for_handle(&lib2);
    store.set_meta("afterSnapshot", "yes").unwrap();
    let mut journal: Value = serde_json::from_slice(&std::fs::read(lib2.root.join("_library_config/migrations.json")).unwrap()).unwrap();
    journal["version"] = json!(5);
    journal["applied"].as_array_mut().unwrap().push(json!({"version": 5, "name": "shared-sources", "appVersion": "x"}));
    std::fs::write(lib2.root.join("_library_config/migrations.json"), serde_json::to_vec(&journal).unwrap()).unwrap();
    let restored = crate::infra::snapshot::restore(&lib2.root, &name).unwrap();
    assert!(restored.previous.is_file(), "the database that was replaced is kept");
    assert_eq!(Store::for_handle(&lib2).meta("afterSnapshot"), None);
    let journal: Value = serde_json::from_slice(&std::fs::read(lib2.root.join("_library_config/migrations.json")).unwrap()).unwrap();
    assert_eq!(journal["version"], 4);
    assert_eq!(journal["applied"].as_array().unwrap().len(), 4);
    assert!(crate::infra::snapshot::restore(&lib2.root, "library-v9-nothing.db").is_err());
}
