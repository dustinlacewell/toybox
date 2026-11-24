//! Substances as assets of their own: `origin.derivedFrom`, the listing of derived assets, transfers that
//! carry the parent, and library migration step 6 (`substance-assets`), which turns every Substance archive
//! a material keeps as a shared source into a substance asset, in place, by moving the archive.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{json, Value};

use super::import::merge_entries;
use super::transfer::{export_package, ingest_package, local_reader, transfer_between, ExportOptions, IngestOptions, OnConflict, Outcome, TransferMode, TransferOptions};
use crate::domain::catalog_model::Asset;
use crate::domain::sources;
use crate::infra::fsio;
use crate::infra::registry::{LibraryEntry, LibraryHandle};
use crate::infra::store::browse::BrowseFilter;
use crate::infra::store::{Delta, ItemPut, Store};
use crate::infra::substance_migration::{self as migration, substance_id};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Base(PathBuf);

impl Base {
    fn new(name: &str) -> Base {
        let dir = std::env::temp_dir().join(format!("toybox-substance-{name}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
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

/// A small PNG of one colour (so the albedo map can be read for a preview).
fn png(width: u32, height: u32, shade: u8) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([shade, 40, 90, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// A material rendered from `archive` the way the plugin before version 2 committed one: the archive staged
/// beside the maps and listed in `origin.files` (the host moves it to `_sources`).
fn old_style(library: &LibraryHandle, name: &str, archive_name: &str, archive: &[u8]) -> Value {
    put(library, &format!("library/_import_s/materials/{name}/color.png"), &png(600, 400, name.len() as u8 * 20));
    put(library, &format!("library/_import_s/materials/{name}/{archive_name}"), archive);
    json!({"id": format!("uid://sbsar_{name}"), "kind": "material", "pack": "_import_s", "category": "tiles", "name": name,
        "maps": [{"role": "albedo", "file": "color.png"}],
        "tags": ["ceramic"],
        "provenance": {"source": "Substance Share", "publisher": "Adobe", "license": "CC0"},
        "origin": {"plugin": "com.toybox.material-sbsar", "files": [archive_name], "meta": {"sbsar": archive_name, "graph": 0, "preset": name, "inputs": {"a": 1}, "size": 9}}})
}

fn all_assets(library: &LibraryHandle) -> Vec<Asset> {
    let store = Store::for_handle(library);
    let mut ids = Vec::new();
    store.for_each_asset_id(&mut |id| ids.push(id.to_string())).unwrap();
    store.load_top_assets(&ids).unwrap()
}

fn asset(library: &LibraryHandle, id: &str) -> Asset {
    let mut found = Store::for_handle(library).load_top_assets(&[id.to_string()]).unwrap();
    crate::ops::content_hash::fill_assets(library, &mut found);
    found.remove(0)
}

fn entry(library: &LibraryHandle, id: &str) -> Option<Value> {
    Store::for_handle(library).load_entries(&[id.to_string()]).unwrap().into_iter().next()
}

fn files_below(folder: &Path) -> Vec<PathBuf> {
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

fn tree_hash(root: &Path) -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = files_below(root)
        .into_iter()
        .filter(|path| !path.to_string_lossy().contains("_library_config"))
        .map(|path| (path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), hash(&std::fs::read(&path).unwrap())))
        .collect();
    all.sort();
    all
}

fn journal_at(library: &LibraryHandle, version: u32) {
    let path = library.root.join("_library_config/migrations.json");
    let mut journal: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    journal["version"] = json!(version);
    journal["applied"].as_array_mut().unwrap().retain(|step| step["version"].as_u64().unwrap() <= version as u64);
    std::fs::write(path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();
}

fn journal_version(library: &LibraryHandle) -> u64 {
    let journal: Value = serde_json::from_slice(&std::fs::read(library.root.join("_library_config/migrations.json")).unwrap()).unwrap();
    journal["version"].as_u64().unwrap()
}

fn open(library: &LibraryHandle) {
    crate::infra::migrations::run(&library.root, &library.cache_path).unwrap();
}

/// A library the way the version before step 6 left it: old-style materials, journal at 5.
fn step_five(base: &Base, entries: impl FnOnce(&LibraryHandle) -> Vec<Value>) -> LibraryHandle {
    let lib = library(base, "a");
    let made = entries(&lib);
    merge_entries(&lib, made).unwrap();
    journal_at(&lib, 5);
    lib
}

fn shared(archive: &[u8], name: &str) -> String {
    sources::shared_path("materials", &hash(archive), name)
}

/// What a conversion must not change about a material.
fn fingerprint(asset: &Asset, entry: &Value) -> Value {
    json!({"id": asset.id, "rel": asset.rel_path, "storage": entry["storagePath"], "maps": entry["maps"], "tags": asset.tags,
        "user": serde_json::to_value(&asset.user).unwrap(), "meta": asset.origin.as_ref().unwrap().meta, "name": asset.name,
        "provenance": entry["provenance"], "hash": asset.content_hash})
}

#[test]
fn materials_sharing_an_archive_become_one_substance_in_place() {
    let base = Base::new("inplace");
    let archive = vec![5u8; 50_000];
    let lib = step_five(&base, |lib| vec![old_style(lib, "Green", "tiles.sbsar", &archive), old_style(lib, "Red", "tiles.sbsar", &archive), old_style(lib, "Blue", "tiles.sbsar", &archive)]);
    let source = shared(&archive, "tiles.sbsar");
    assert!(lib.root.join(&source).is_file());
    // Personal data on one of them, which must survive.
    let store = Store::for_handle(&lib);
    let mut green = asset(&lib, "uid://sbsar_Green");
    green.user.favorite = true;
    green.user.collections = vec!["Floors".into()];
    store.apply(&Delta { puts: vec![ItemPut { id: green.id.clone(), entry: None, assets: vec![green], rigs: vec![] }], ..Default::default() }).unwrap();
    crate::ops::content_hash::hash_new(&lib, &mut all_assets(&lib));
    assert!(asset(&lib, "uid://sbsar_Red").content_hash.is_some());
    let before: Vec<Value> = ["Green", "Red", "Blue"].iter().map(|n| format!("uid://sbsar_{n}")).map(|id| fingerprint(&asset(&lib, &id), &entry(&lib, &id).unwrap())).collect();
    let thumbs_before = std::fs::read_dir(&lib.thumbs_dir).map(|read| read.count()).unwrap_or(0);

    open(&lib);

    assert_eq!(journal_version(&lib), 6);
    let substance = substance_id(&hash(&archive));
    assert_eq!(substance.len(), "uid://substance_".len() + 32);
    let stored = entry(&lib, &substance).expect("one substance for the archive");
    assert_eq!(stored["kind"], "plugin");
    assert_eq!(stored["type"], "substance");
    assert_eq!(stored["name"], "tiles", "several materials use it: the archive's name");
    assert_eq!(stored["category"], "tiles");
    assert_eq!(stored["format"], "sbsar");
    assert_eq!(stored["tags"], json!(["ceramic"]));
    assert_eq!(stored["provenance"]["source"], "Substance Share");
    assert_eq!(stored["origin"], json!({"plugin": "com.toybox.material-sbsar"}));
    assert_eq!(stored["meta"], json!({"sbsar": "tiles.sbsar", "sha256": hash(&archive), "bytes": 50_000}));
    assert_eq!(stored["files"], json!(["tiles.sbsar", "preview.png"]));
    let folder = stored["storagePath"].as_str().unwrap().to_string();
    assert!(folder.starts_with("library/substance/"), "{folder}");
    // The archive was moved, not copied: it is in the substance's folder and nowhere else.
    assert_eq!(std::fs::read(lib.root.join(&folder).join("tiles.sbsar")).unwrap(), archive);
    assert!(!lib.root.join(&source).exists());
    assert!(files_below(&lib.root.join("library/materials/_sources")).is_empty(), "the emptied folder went too");
    assert_eq!(files_below(&lib.root.join("library")).iter().filter(|p| p.extension().is_some_and(|e| e == "sbsar")).count(), 1);
    // The preview: a square PNG of at most 512 pixels from the first material's albedo map.
    let preview = image::open(lib.root.join(&folder).join("preview.png")).unwrap();
    assert_eq!((preview.width(), preview.height()), (400, 400));
    // The substance is a catalog asset with a content hash computed without reading the archive again.
    let sub = asset(&lib, &substance);
    assert_eq!(sub.content.type_id(), "substance");
    let mut hasher = crate::ops::content_hash::ContentHasher::new(&lib.root);
    let read_hash = hasher.hash_asset(&sub).unwrap();
    let known = crate::infra::store::hashes::get_keys(&lib.library_db_path(), &[crate::ops::content_hash::storage_key(&sub).unwrap()]);
    assert_eq!(known.values().next(), Some(&read_hash), "the assumed digest is the archive's real one");

    // The materials: linked, no archive, `format` set, nothing else changed.
    for (index, name) in ["Green", "Red", "Blue"].iter().enumerate() {
        let id = format!("uid://sbsar_{name}");
        let (material, stored) = (asset(&lib, &id), entry(&lib, &id).unwrap());
        assert_eq!(fingerprint(&material, &stored), before[index], "{name} is the same material");
        assert_eq!(stored["origin"]["derivedFrom"], json!(substance));
        assert!(stored["origin"].get("files").is_none());
        assert_eq!(stored["format"], "sbsar");
        let origin = material.origin.unwrap();
        assert_eq!(origin.derived_from.as_deref(), Some(substance.as_str()));
        assert!(origin.files.is_empty());
        assert_eq!(material.format.as_deref(), Some("sbsar"));
    }
    assert!(asset(&lib, "uid://sbsar_Green").user.favorite);
    assert_eq!(std::fs::read_dir(&lib.thumbs_dir).map(|read| read.count()).unwrap_or(0), thumbs_before);
    // Listed as derived assets.
    let mut derived = store.derived_from(&substance).unwrap();
    derived.sort();
    assert_eq!(derived, vec!["uid://sbsar_Blue", "uid://sbsar_Green", "uid://sbsar_Red"]);

    let report = migration::report(&store).unwrap();
    assert_eq!((report.substances, report.materials, report.archives_moved, report.bytes_moved, report.previews), (1, 3, 1, 50_000, 1));
    assert!(report.skipped.is_empty());
    assert!(!migration::pending_journal(&lib.root).exists());
    // Again: nothing changes.
    let tree = tree_hash(&lib.root);
    let revision = store.stamp().unwrap();
    open(&lib);
    assert_eq!(tree_hash(&lib.root), tree);
    assert_eq!(store.stamp().unwrap(), revision);
    assert_eq!(migration::report(&store).unwrap(), report);
    // A rescan from the manifest finds everything where the entries say.
    let rescanned = super::scan::scan_library_catalog(&lib).unwrap();
    assert_eq!(rescanned.assets.len(), 4);
}

#[test]
fn a_lone_material_names_its_substance_and_other_sources_and_models_are_untouched() {
    let base = Base::new("rules");
    let (archive, other) = (vec![1u8; 3000], vec![2u8; 3000]);
    let lib = step_five(&base, |lib| {
        // A material with a Substance archive and a second kept source.
        let mut both = old_style(lib, "Both", "rock.SBSAR", &archive);
        put(lib, "library/_import_s/materials/Both/notes.txt", b"notes");
        both["origin"]["files"] = json!(["rock.SBSAR", "notes.txt"]);
        // A material whose only source is not an archive.
        let mut plain = old_style(lib, "Plain", "x.sbsar", &other);
        put(lib, "library/_import_s/materials/Plain/scan.raw", b"raw");
        plain["origin"]["files"] = json!(["scan.raw"]);
        // A model that keeps its original.
        put(lib, "library/_import_s/props/Crate.gltf", br#"{"asset":{"version":"2.0"},"nodes":[],"buffers":[]}"#);
        put(lib, "library/_import_s/props/crate.sbsar", b"not a material's archive");
        let model = json!({"id": "uid://crate", "pack": "_import_s", "category": "props", "file": "Crate.gltf", "origin": {"plugin": "x", "files": ["crate.sbsar"]}});
        vec![both, plain, model]
    });
    let plain_before = entry(&lib, "uid://sbsar_Plain").unwrap();
    let model_before = entry(&lib, "uid://crate").unwrap();
    assert!(sources::is_shared(model_before["origin"]["files"][0].as_str().unwrap()));
    open(&lib);
    let substance = substance_id(&hash(&archive));
    let stored = entry(&lib, &substance).unwrap();
    assert_eq!(stored["name"], "Both", "only one material uses it: its name");
    assert_eq!(stored["meta"]["sbsar"], "rock.SBSAR");
    let both = entry(&lib, "uid://sbsar_Both").unwrap();
    assert_eq!(both["origin"]["files"].as_array().unwrap().len(), 1, "the other source stays");
    assert!(both["origin"]["files"][0].as_str().unwrap().ends_with("/notes.txt"));
    assert_eq!(both["origin"]["derivedFrom"], json!(substance));
    assert_eq!(entry(&lib, "uid://sbsar_Plain").unwrap(), plain_before);
    assert_eq!(entry(&lib, "uid://crate").unwrap(), model_before);
    assert!(lib.root.join(model_before["origin"]["files"][0].as_str().unwrap()).is_file());
    assert!(entry(&lib, &substance_id(&hash(&other))).is_none(), "an archive nothing lists is not a substance");
}

#[test]
fn variants_are_linked_and_count_as_materials() {
    let base = Base::new("variants");
    let archive = vec![3u8; 2000];
    let lib = step_five(&base, |lib| {
        let mut top = old_style(lib, "Top", "v.sbsar", &archive);
        put(lib, "library/_import_s/materials/Top_1K/color.png", &png(64, 64, 9));
        put(lib, "library/_import_s/materials/Top_1K/v.sbsar", &archive);
        top["variants"] = json!([{"id": "uid://sbsar_Top_1K", "kind": "material", "pack": "_import_s", "category": "tiles", "name": "Top_1K",
            "maps": [{"role": "albedo", "file": "color.png"}], "origin": {"plugin": "com.toybox.material-sbsar", "files": ["v.sbsar"], "meta": {"sbsar": "v.sbsar"}}}]);
        vec![top]
    });
    open(&lib);
    let substance = substance_id(&hash(&archive));
    assert_eq!(entry(&lib, &substance).unwrap()["name"], "v", "a material and its variant: two users");
    let top = entry(&lib, "uid://sbsar_Top").unwrap();
    assert_eq!(top["variants"][0]["origin"]["derivedFrom"], json!(substance));
    assert!(top["variants"][0]["origin"].get("files").is_none());
    assert_eq!(top["variants"][0]["format"], "sbsar");
    let material = asset(&lib, "uid://sbsar_Top");
    assert_eq!(material.variants[0].origin.as_ref().unwrap().derived_from.as_deref(), Some(substance.as_str()));
}

/// Put an archive in flight the way a run that stopped after moving it would leave it.
fn stop_after_the_move(lib: &LibraryHandle, archive: &[u8], name: &str, copy: bool) -> (String, String) {
    let from = shared(archive, name);
    let to = format!("library/substance/{}/{name}", "c".repeat(64));
    std::fs::create_dir_all(lib.root.join(&to).parent().unwrap()).unwrap();
    let pending = json!({"from": from, "to": to, "created": [format!("library/substance/{}/preview.png", "c".repeat(64))], "substance": substance_id(&hash(archive)), "storagePath": format!("library/substance/{}", "c".repeat(64))});
    fsio::write_bytes(&migration::pending_journal(&lib.root), &serde_json::to_vec(&pending).unwrap()).unwrap();
    put(lib, &format!("library/substance/{}/preview.png", "c".repeat(64)), b"half a preview");
    if copy {
        std::fs::copy(lib.root.join(&from), lib.root.join(&to)).unwrap();
    } else {
        std::fs::rename(lib.root.join(&from), lib.root.join(&to)).unwrap();
    }
    (from, to)
}

#[test]
fn a_run_stopped_at_any_point_is_settled_by_the_next_open() {
    for copy in [false, true] {
        let base = Base::new("stopped");
        let archive = vec![8u8; 20_000];
        let lib = step_five(&base, |lib| vec![old_style(lib, "One", "s.sbsar", &archive), old_style(lib, "Two", "s.sbsar", &archive)]);
        // Step 6 is journaled (an older Toybox refuses the library), the archive moved, nothing committed.
        journal_at(&lib, 6);
        Store::for_handle(&lib).set_meta(migration::META_DONE, "done").unwrap();
        let (from, to) = stop_after_the_move(&lib, &archive, "s.sbsar", copy);
        // Until the next open the entries name the `_sources` path; with a rename it is gone meanwhile.
        assert_eq!(lib.root.join(&from).is_file(), copy);
        open(&lib);
        assert!(!migration::pending_journal(&lib.root).exists());
        assert!(!lib.root.join(&to).exists() && !lib.root.join(Path::new(&to).parent().unwrap()).exists(), "what the stopped run made is gone");
        let substance = substance_id(&hash(&archive));
        let stored = entry(&lib, &substance).expect("converted by the open");
        assert_eq!(std::fs::read(lib.root.join(stored["storagePath"].as_str().unwrap()).join("s.sbsar")).unwrap(), archive);
        assert!(!lib.root.join(&from).exists());
        for id in ["uid://sbsar_One", "uid://sbsar_Two"] {
            assert_eq!(entry(&lib, id).unwrap()["origin"]["derivedFrom"], json!(substance));
        }
    }
}

#[test]
fn a_run_stopped_after_the_commit_only_cleans_up() {
    let base = Base::new("committed");
    let archive = vec![4u8; 10_000];
    let lib = step_five(&base, |lib| vec![old_style(lib, "One", "s.sbsar", &archive)]);
    open(&lib);
    let substance = substance_id(&hash(&archive));
    let stored = entry(&lib, &substance).unwrap();
    let folder = stored["storagePath"].as_str().unwrap().to_string();
    let to = format!("{folder}/s.sbsar");
    // Stopped after the commit, before the cleanup, with the copy fallback's original still there.
    let from = shared(&archive, "s.sbsar");
    put(&lib, &from, &archive);
    let pending = json!({"from": from, "to": to, "created": [format!("{folder}/preview.png")], "substance": substance, "storagePath": folder});
    fsio::write_bytes(&migration::pending_journal(&lib.root), &serde_json::to_vec(&pending).unwrap()).unwrap();
    let tree_before: Vec<_> = tree_hash(&lib.root).into_iter().filter(|(path, _)| !path.contains("_sources")).collect();
    open(&lib);
    assert!(!migration::pending_journal(&lib.root).exists());
    assert!(!lib.root.join(&from).exists(), "the leftover original went");
    assert!(lib.root.join(&to).is_file() && lib.root.join(&folder).join("preview.png").is_file(), "the committed files stay");
    assert_eq!(tree_hash(&lib.root), tree_before);
}

#[test]
fn an_archive_that_cannot_be_moved_stays_with_its_materials_and_is_tried_again() {
    let base = Base::new("missing");
    let (fine, broken) = (vec![1u8; 100], vec![2u8; 100]);
    let lib = step_five(&base, |lib| vec![old_style(lib, "Fine", "a.sbsar", &fine), old_style(lib, "Broken", "b.sbsar", &broken)]);
    let gone = shared(&broken, "b.sbsar");
    std::fs::rename(lib.root.join(&gone), base.0.join("aside.sbsar")).unwrap();
    open(&lib);
    let store = Store::for_handle(&lib);
    let report = migration::report(&store).unwrap();
    assert_eq!(report.substances, 1);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].0, gone);
    let left = entry(&lib, "uid://sbsar_Broken").unwrap();
    assert_eq!(left["origin"]["files"], json!([gone]), "left as it was");
    assert!(left["origin"].get("derivedFrom").is_none());
    assert!(entry(&lib, &substance_id(&hash(&fine))).is_some());
    // The file is back: the next open converts it. (Its emptied set folder may have been swept as
    // empty meanwhile by the open-time tidy-up, which is allowed: the file comes back with its folder.)
    std::fs::create_dir_all(lib.root.join(&gone).parent().unwrap()).unwrap();
    std::fs::rename(base.0.join("aside.sbsar"), lib.root.join(&gone)).unwrap();
    open(&lib);
    let report = migration::report(&store).unwrap();
    assert_eq!(report.substances, 2);
    assert!(report.skipped.is_empty());
    assert_eq!(entry(&lib, "uid://sbsar_Broken").unwrap()["origin"]["derivedFrom"], json!(substance_id(&hash(&broken))));
}

#[test]
fn materials_committed_old_style_later_are_linked_by_the_next_open() {
    let base = Base::new("stragglers");
    let archive = vec![6u8; 4000];
    let lib = step_five(&base, |lib| vec![old_style(lib, "First", "t.sbsar", &archive)]);
    open(&lib);
    let substance = substance_id(&hash(&archive));
    let folder = entry(&lib, &substance).unwrap()["storagePath"].as_str().unwrap().to_string();
    // An older client commits two more old-style materials: one of the same archive, one of a new archive.
    let fresh = vec![7u8; 4000];
    merge_entries(&lib, vec![old_style(&lib, "Second", "t.sbsar", &archive), old_style(&lib, "Third", "u.sbsar", &fresh)]).unwrap();
    assert!(lib.root.join(shared(&archive, "t.sbsar")).is_file(), "the host stored the archive as a shared source again");
    open(&lib);
    assert_eq!(entry(&lib, "uid://sbsar_Second").unwrap()["origin"]["derivedFrom"], json!(substance));
    assert!(!lib.root.join(shared(&archive, "t.sbsar")).exists(), "the duplicate copy went");
    assert_eq!(entry(&lib, &substance).unwrap()["storagePath"], json!(folder), "the substance is not stored again");
    assert!(lib.root.join(&folder).join("t.sbsar").is_file());
    assert_eq!(entry(&lib, "uid://sbsar_Third").unwrap()["origin"]["derivedFrom"], json!(substance_id(&hash(&fresh))));
    let report = migration::report(&Store::for_handle(&lib)).unwrap();
    assert_eq!((report.substances, report.materials, report.duplicates_removed), (2, 3, 1));
    // Nothing left: an open does not look again until the library changes.
    assert!(!migration::pending(&lib.root, &lib.cache_path).unwrap());
}

#[test]
fn the_pre_flight_reports_step_six_and_a_snapshot_is_taken_first() {
    let base = Base::new("preflight");
    let archive = vec![9u8; 7000];
    let lib = step_five(&base, |lib| vec![old_style(lib, "A", "p.sbsar", &archive), old_style(lib, "B", "p.sbsar", &archive), old_style(lib, "C", "q.sbsar", &[1u8; 500])]);
    let before = tree_hash(&lib.root);
    let report = crate::infra::preflight::check(&lib.root).unwrap();
    assert_eq!(tree_hash(&lib.root), before);
    assert_eq!((report.format_version, report.latest_version), (5, 6));
    let step = report.pending.iter().find(|step| step.version == 6).unwrap();
    assert_eq!(step.name, "substance-assets");
    assert_eq!((step.items, step.files, step.bytes), (Some(3), Some(2), Some(7500)));
    assert!(step.rewrites_entries);
    assert!(report.snapshot.as_deref().is_some_and(|path| path.replace('\\', "/").contains("library-v5-")));
    open(&lib);
    let made = crate::infra::snapshot::list(&lib.root);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].from_version, 5);
    let connection = crate::infra::snapshot::open_unchanged(&made[0].path).unwrap();
    let old: String = connection.query_row("SELECT entry FROM items WHERE id = 'uid://sbsar_A'", [], |row| row.get(0)).unwrap();
    assert!(old.contains("_sources") && !old.contains("derivedFrom"), "{old}");
    drop(connection);
    assert!(crate::infra::preflight::check(&lib.root).unwrap().pending.is_empty());
}

#[test]
fn derived_assets_are_listed_through_the_browse_filter() {
    let base = Base::new("browse");
    let archive = vec![2u8; 900];
    let lib = step_five(&base, |lib| vec![old_style(lib, "Left", "b.sbsar", &archive), old_style(lib, "Right", "b.sbsar", &archive), old_style(lib, "Other", "c.sbsar", &[5u8; 9])]);
    open(&lib);
    let substance = substance_id(&hash(&archive));
    let libraries = vec![crate::ops::browse::BrowseLibrary { handle: lib.clone(), rank: 0 }];
    // Any type: `kind` empty.
    let filter = BrowseFilter { derived_from: Some(substance.clone()), ..Default::default() };
    let page = crate::ops::browse::query_assets(&libraries, &filter, false, 0, 50).unwrap();
    let names: Vec<&str> = page.rows.iter().map(|row| row.asset.name.as_str()).collect();
    assert_eq!(names, vec!["Left", "Right"]);
    assert_eq!(page.total, 2);
    assert_eq!(crate::ops::browse::count_assets(&libraries, &filter).unwrap(), 2);
    assert_eq!(crate::ops::browse::query_ids(&libraries, &filter, false, 0, 10).unwrap().ids.len(), 2);
    // With a type.
    let typed = BrowseFilter { kind: "material".into(), derived_from: Some(substance.clone()), ..Default::default() };
    assert_eq!(crate::ops::browse::count_assets(&libraries, &typed).unwrap(), 2);
    let wrong = BrowseFilter { kind: "substance".into(), derived_from: Some(substance.clone()), ..Default::default() };
    assert_eq!(crate::ops::browse::count_assets(&libraries, &wrong).unwrap(), 0);
    // The serde name the window sends.
    let parsed: BrowseFilter = serde_json::from_value(json!({"kind": "", "derivedFrom": substance})).unwrap();
    assert_eq!(parsed, filter);
    let without: BrowseFilter = serde_json::from_value(json!({"kind": "material"})).unwrap();
    assert_eq!(without.derived_from, None);
    // Deleting the parent leaves the children, whose link dangles.
    crate::ops::delete::delete_from_library(&lib.root, &lib.cache_path, std::slice::from_ref(&substance)).unwrap();
    assert!(entry(&lib, "uid://sbsar_Left").is_some());
    assert_eq!(Store::for_handle(&lib).derived_from(&substance).unwrap().len(), 2);
}

#[test]
fn a_copy_carries_the_substance_once_and_a_move_leaves_it() {
    let base = Base::new("transfer");
    let archive = vec![4u8; 3000];
    let from = step_five(&base, |lib| vec![old_style(lib, "A", "t.sbsar", &archive), old_style(lib, "B", "t.sbsar", &archive)]);
    open(&from);
    let to = library(&base, "to");
    let substance = substance_id(&hash(&archive));
    let ids = vec!["uid://sbsar_A".to_string(), "uid://sbsar_B".to_string()];
    let package = export_package(&from, &ids, ExportOptions::default()).unwrap();
    assert_eq!(package.assets.iter().map(|a| (a.id.as_str(), a.carried)).collect::<Vec<_>>(), vec![(substance.as_str(), true), ("uid://sbsar_A", false), ("uid://sbsar_B", false)]);
    let report = ingest_package(&to, &package, IngestOptions::default(), &local_reader(&from), &|_| {}).unwrap();
    assert!(report.items.iter().all(|item| item.outcome == Outcome::Copied), "{report:?}");
    assert!(report.items.iter().find(|item| item.id == substance).unwrap().carried);
    let copied = entry(&to, &substance).unwrap();
    assert_eq!(std::fs::read(to.root.join(copied["storagePath"].as_str().unwrap()).join("t.sbsar")).unwrap(), archive);
    assert_eq!(entry(&to, "uid://sbsar_A").unwrap()["origin"]["derivedFrom"], json!(substance));
    assert_eq!(Store::for_handle(&to).derived_from(&substance).unwrap().len(), 2);
    // Again, replacing: the substance the target has is neither replaced nor reported.
    let package = export_package(&from, &ids[..1], ExportOptions::default()).unwrap();
    let report = ingest_package(&to, &package, IngestOptions { on_conflict: OnConflict::Replace }, &local_reader(&from), &|_| {}).unwrap();
    assert_eq!(report.items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), vec!["uid://sbsar_A"]);
    assert_eq!(entry(&to, &substance).unwrap(), copied);
    // A move takes the materials and leaves the substance in the source.
    let third = library(&base, "third");
    let report = transfer_between(&from, &third, &ids, TransferOptions { mode: TransferMode::Move, on_conflict: OnConflict::Skip, include_clips: false }, &|_| {}).unwrap();
    assert!(report.items.iter().filter(|item| !item.carried).all(|item| item.source_removed), "{report:?}");
    assert!(entry(&from, &substance).is_some(), "the substance stays in the source");
    assert!(entry(&from, "uid://sbsar_A").is_none());
    assert!(entry(&third, &substance).is_some());
}

#[test]
fn derived_from_is_not_part_of_the_storage_key_or_the_content_hash() {
    let base = Base::new("key");
    let lib = library(&base, "a");
    let stage = |name: &str| {
        put(&lib, &format!("library/_import_k{name}/materials/M/color.png"), &png(8, 8, 1));
        json!({"id": "uid://same", "kind": "material", "pack": format!("_import_k{name}"), "category": "c", "name": "M", "maps": [{"role": "albedo", "file": "color.png"}], "origin": {"plugin": "p"}})
    };
    let canonical = lib.root.canonicalize().unwrap();
    let plain = crate::infra::storage::ItemPlan::new(&canonical, vec![stage("1")]).unwrap().entries()[0]["storagePath"].clone();
    // The same entry (planning writes nothing), now linked to the asset it was made from.
    let mut linked = stage("1");
    linked["origin"]["derivedFrom"] = json!("uid://substance_0123");
    let keyed = crate::infra::storage::ItemPlan::new(&canonical, vec![linked]).unwrap().entries()[0]["storagePath"].clone();
    assert_eq!(plain, keyed);
    // The content hash ignores origin altogether.
    merge_entries(&lib, vec![stage("3")]).unwrap();
    let mut material = asset(&lib, "uid://same");
    let mut hasher = crate::ops::content_hash::ContentHasher::new(&lib.root);
    let before = hasher.hash_asset(&material).unwrap();
    material.origin.as_mut().unwrap().derived_from = Some("uid://substance_0123".into());
    assert_eq!(hasher.hash_asset(&material).unwrap(), before);
}

#[test]
fn derived_from_is_validated_and_kept() {
    let base = |origin: Value| json!({"id": "m", "kind": "plugin", "type": "decal", "pack": "p", "category": "c", "name": "D", "files": ["a.png"], "origin": origin});
    assert!(crate::domain::seed::parse_entry(&base(json!({"plugin": "p", "derivedFrom": 3}))).is_err());
    assert!(crate::domain::seed::parse_entry(&base(json!({"plugin": "p", "derivedFrom": ""}))).is_err());
    assert!(crate::domain::seed::parse_entry(&base(json!({"plugin": "p", "derivedFrom": "lib::uid://x"}))).is_err(), "raw ids in a manifest");
    let parsed = crate::domain::seed::parse_entry(&base(json!({"plugin": "p", "derivedFrom": "uid://x"}))).unwrap();
    assert_eq!(parsed.origin.unwrap().derived_from.as_deref(), Some("uid://x"));
    let origin = crate::domain::catalog_model::AssetOrigin::from_entry(&json!({"origin": {"plugin": "p", "derivedFrom": "uid://x"}})).unwrap();
    assert_eq!(serde_json::to_value(&origin).unwrap(), json!({"plugin": "p", "derivedFrom": "uid://x"}));
    let none = crate::domain::catalog_model::AssetOrigin::from_entry(&json!({"origin": {"plugin": "p"}})).unwrap();
    assert_eq!(serde_json::to_value(&none).unwrap(), json!({"plugin": "p"}));
}

#[test]
fn a_database_of_schema_one_gains_the_derived_column_in_place() {
    let base = Base::new("schema");
    let archive = vec![3u8; 300];
    let lib = step_five(&base, |lib| vec![old_style(lib, "Old", "o.sbsar", &archive)]);
    open(&lib);
    let substance = substance_id(&hash(&archive));
    // Make it a version 1 database: no column, no index.
    {
        let connection = rusqlite::Connection::open(lib.library_db_path()).unwrap();
        connection.execute_batch("DROP INDEX assets_derived_from; ALTER TABLE assets DROP COLUMN derived_from; PRAGMA user_version=1;").unwrap();
    }
    let store = Store::for_handle(&lib);
    assert_eq!(store.derived_from(&substance).unwrap(), vec!["uid://sbsar_Old"], "filled from the stored assets on open");
    let connection = rusqlite::Connection::open(lib.library_db_path()).unwrap();
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
    assert_eq!(version, crate::infra::store::SCHEMA_VERSION);
}

// ---- step 6 under failure ---------------------------------------------------------------------------

use crate::infra::substance_migration::hooks;

const ITEMS_JOURNAL: &str = "_library_config/storage-pending-items.json";

fn set(cell: &'static std::thread::LocalKey<std::cell::Cell<u32>>, value: u32) {
    cell.with(|c| c.set(value));
}

fn get(cell: &'static std::thread::LocalKey<std::cell::Cell<u32>>) -> u32 {
    cell.with(|c| c.get())
}

fn sbsar_files(lib: &LibraryHandle) -> Vec<PathBuf> {
    files_below(&lib.root.join("library")).into_iter().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sbsar"))).collect()
}

/// The archive is reachable from the substance, and nothing is left in `_sources`.
fn converged(lib: &LibraryHandle, archive: &[u8], name: &str) {
    let substance = substance_id(&hash(archive));
    let stored = entry(lib, &substance).expect("the substance");
    assert_eq!(std::fs::read(lib.root.join(stored["storagePath"].as_str().unwrap()).join(name)).unwrap(), archive);
    assert!(files_below(&lib.root.join("library/materials/_sources")).is_empty(), "{:?}", files_below(&lib.root.join("library/materials/_sources")));
    assert!(!migration::pending_journal(&lib.root).exists());
    assert!(!lib.root.join(ITEMS_JOURNAL).exists());
    super::scan::scan_library_catalog(lib).expect("the catalog builds");
}

#[test]
fn a_commit_that_fails_after_its_journal_is_finished_before_anything_is_undone() {
    // Once: the journal is finished at once (the commit lands) instead of the archive being moved back.
    let base = Base::new("applyonce");
    let archive = vec![11u8; 30_000];
    let lib = step_five(&base, |lib| vec![old_style(lib, "One", "f.sbsar", &archive)]);
    set(&crate::infra::storage::FAIL_APPLY, 1);
    open(&lib);
    assert_eq!(get(&crate::infra::storage::FAIL_APPLY), 0, "the failure happened");
    converged(&lib, &archive, "f.sbsar");
    assert_eq!(entry(&lib, "uid://sbsar_One").unwrap()["origin"]["derivedFrom"], json!(substance_id(&hash(&archive))));
    assert_eq!(migration::report(&Store::for_handle(&lib)).unwrap().substances, 1);
}

#[test]
fn a_commit_that_cannot_be_finished_stops_the_run_and_the_next_open_converges() {
    let base = Base::new("applytwice");
    let mut pair = vec![vec![12u8; 30_000], vec![13u8; 1000]];
    pair.sort_by_key(|bytes| hash(bytes));
    let (archive, second) = (pair[0].clone(), pair[1].clone());
    let lib = step_five(&base, |lib| vec![old_style(lib, "One", "f.sbsar", &archive), old_style(lib, "Two", "g.sbsar", &second)]);
    // The commit fails, and so does finishing its journal (the disk is still full).
    set(&crate::infra::storage::FAIL_APPLY, 2);
    open(&lib); // the open goes on
    assert_eq!(journal_version(&lib), 6);
    assert!(lib.root.join(ITEMS_JOURNAL).exists(), "the publication journal is kept");
    assert!(migration::pending_journal(&lib.root).exists(), "and the step's");
    // Nothing was undone and nothing is missing: every path an entry names is there.
    assert!(lib.root.join(shared(&archive, "f.sbsar")).is_file() && lib.root.join(shared(&second, "g.sbsar")).is_file());
    assert_eq!(sbsar_files(&lib).len(), 3, "the two archives, and the link of the first in its substance's folder");
    // The run stopped: one conversion at a time, no second journal.
    assert!(Store::for_handle(&lib).load_entries(&[substance_id(&hash(&second)), substance_id(&hash(&archive))]).unwrap().is_empty());
    // The next open finishes the publication first, then the step.
    open(&lib);
    converged(&lib, &archive, "f.sbsar");
    assert!(entry(&lib, &substance_id(&hash(&second))).is_some());
    assert_eq!(entry(&lib, "uid://sbsar_One").unwrap()["origin"]["derivedFrom"], json!(substance_id(&hash(&archive))));
    assert_eq!(sbsar_files(&lib).len(), 2);
}

/// Two archives whose groups run in this order (by path), the first not matching its name.
fn mismatch_first(base: &Base) -> (LibraryHandle, Vec<u8>, Vec<u8>, String) {
    let mut pair = vec![vec![21u8; 4000], vec![22u8; 4000]];
    pair.sort_by_key(|bytes| hash(bytes));
    let (bad, good) = (pair[0].clone(), pair[1].clone());
    let lib = step_five(base, |lib| vec![old_style(lib, "Bad", "bad.sbsar", &bad), old_style(lib, "Good", "good.sbsar", &good)]);
    let bad_path = shared(&bad, "bad.sbsar");
    std::fs::write(lib.root.join(&bad_path), b"damaged, and of another length").unwrap();
    (lib, bad, good, bad_path)
}

#[test]
fn a_rollback_that_fails_stops_the_run_with_its_journal_and_the_next_open_converges() {
    let base = Base::new("recoverfail");
    let (lib, _bad, good, bad_path) = mismatch_first(&base);
    set(&hooks::FAIL_RECOVER, 1);
    open(&lib);
    let journal = std::fs::read(migration::pending_journal(&lib.root)).expect("the journal of the first archive is kept");
    assert!(String::from_utf8_lossy(&journal).contains(&bad_path));
    assert!(entry(&lib, &substance_id(&hash(&good))).is_none(), "the next archive waits: its journal would overwrite this one");
    // The library still reads: every path an entry names is there.
    assert!(lib.root.join(&bad_path).is_file() && lib.root.join(shared(&good, "good.sbsar")).is_file());
    super::scan::scan_library_catalog(&lib).expect("the catalog builds");
    open(&lib);
    assert!(!migration::pending_journal(&lib.root).exists());
    let report = migration::report(&Store::for_handle(&lib)).unwrap();
    assert_eq!(report.substances, 1);
    assert_eq!(report.skipped.len(), 1);
    assert!(report.skipped[0].1.contains("does not match its name"), "{:?}", report.skipped);
    assert!(entry(&lib, &substance_id(&hash(&good))).is_some());
    assert_eq!(sbsar_files(&lib).len(), 2, "the damaged archive stays where it was, the good one is in its substance");
}

#[test]
fn an_archive_that_does_not_match_its_name_is_skipped_once_and_not_read_again() {
    let base = Base::new("mismatch");
    let (lib, bad, _good, bad_path) = mismatch_first(&base);
    let before = entry(&lib, "uid://sbsar_Bad").unwrap();
    open(&lib);
    assert_eq!(entry(&lib, "uid://sbsar_Bad").unwrap(), before, "its material is untouched");
    assert!(lib.root.join(&bad_path).is_file());
    assert!(entry(&lib, &substance_id(&hash(&bad))).is_none(), "no substance with a false content hash");
    let report = migration::report(&Store::for_handle(&lib)).unwrap();
    assert_eq!((report.substances, report.skipped.len(), report.skipped_files.len()), (1, 1, 1));
    // Remembered: neither an open nor a write scans the entries or reads the archive again.
    set(&hooks::SCANS, 0);
    set(&hooks::HASHES, 0);
    open(&lib);
    assert!(!migration::pending(&lib.root, &lib.cache_path).unwrap());
    assert_eq!((get(&hooks::SCANS), get(&hooks::HASHES)), (0, 0));
    // A write (of an old-style material of a new archive): one scan, and only the new archive is read.
    merge_entries(&lib, vec![old_style(&lib, "Unrelated", "u.sbsar", &[1u8; 10])]).unwrap();
    open(&lib);
    assert_eq!((get(&hooks::SCANS), get(&hooks::HASHES)), (1, 1));
    assert!(entry(&lib, &substance_id(&hash(&[1u8; 10]))).is_some());
    // Writes that add no archive: no scan, no read.
    set(&hooks::SCANS, 0);
    set(&hooks::HASHES, 0);
    let mut good = asset(&lib, "uid://sbsar_Good");
    good.user.favorite = true;
    Store::for_handle(&lib).apply(&Delta { puts: vec![ItemPut { id: good.id.clone(), entry: None, assets: vec![good], rigs: vec![] }], ..Default::default() }).unwrap();
    put(&lib, "library/_import_w/materials/W/color.png", &png(8, 8, 5));
    merge_entries(&lib, vec![json!({"id": "uid://w", "kind": "material", "pack": "_import_w", "category": "c", "name": "W", "maps": [{"role": "albedo", "file": "color.png"}]})]).unwrap();
    open(&lib);
    open(&lib);
    assert_eq!((get(&hooks::SCANS), get(&hooks::HASHES)), (0, 0));
    // Changed on disk: tried again.
    std::fs::write(lib.root.join(&bad_path), &bad).unwrap();
    assert!(migration::pending(&lib.root, &lib.cache_path).unwrap());
    open(&lib);
    assert!(entry(&lib, &substance_id(&hash(&bad))).is_some());
    assert!(migration::report(&Store::for_handle(&lib)).unwrap().skipped.is_empty());
}

#[test]
fn a_cleanup_that_fails_after_the_commit_is_a_warning() {
    let base = Base::new("cleanup");
    let archive = vec![31u8; 5000];
    let lib = step_five(&base, |lib| vec![old_style(lib, "One", "c.sbsar", &archive)]);
    set(&hooks::FAIL_CLEANUP, 1);
    open(&lib);
    let report = migration::report(&Store::for_handle(&lib)).unwrap();
    assert_eq!(report.substances, 1);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(report.warnings.len(), 1);
    assert!(!migration::pending_journal(&lib.root).exists());
    let source = lib.root.join(shared(&archive, "c.sbsar"));
    assert!(source.is_file(), "the extra name is left");
    // It is only a name: removing it (as `check --fix` does with an orphan) loses nothing.
    std::fs::remove_file(&source).unwrap();
    crate::infra::retention::remove_set_folder_if_empty(&source);
    converged(&lib, &archive, "c.sbsar");
}

#[test]
fn without_hard_links_the_archive_is_renamed() {
    let base = Base::new("nolinks");
    let archive = vec![32u8; 5000];
    let lib = step_five(&base, |lib| vec![old_style(lib, "One", "r.sbsar", &archive), old_style(lib, "Two", "r.sbsar", &archive)]);
    hooks::NO_LINKS.with(|c| c.set(true));
    open(&lib);
    hooks::NO_LINKS.with(|c| c.set(false));
    converged(&lib, &archive, "r.sbsar");
    assert_eq!(sbsar_files(&lib).len(), 1);
}

#[test]
fn a_material_made_from_two_archives_links_the_first_and_both_become_substances() {
    let base = Base::new("two");
    let mut pair = vec![vec![41u8; 3000], vec![42u8; 3000]];
    // The archive listed first is the one whose group runs second.
    pair.sort_by_key(|bytes| std::cmp::Reverse(hash(bytes)));
    let (first, later) = (pair[0].clone(), pair[1].clone());
    let lib = step_five(&base, |lib| {
        let mut both = old_style(lib, "Both", "first.sbsar", &first);
        put(lib, "library/_import_s/materials/Both/later.sbsar", &later);
        both["origin"]["files"] = json!(["first.sbsar", "later.sbsar"]);
        vec![both]
    });
    open(&lib);
    let (one, two) = (substance_id(&hash(&first)), substance_id(&hash(&later)));
    let both = entry(&lib, "uid://sbsar_Both").unwrap();
    assert_eq!(both["origin"]["derivedFrom"], json!(one));
    assert!(both["origin"].get("files").is_none());
    assert_eq!(asset(&lib, "uid://sbsar_Both").origin.unwrap().derived_from.as_deref(), Some(one.as_str()));
    assert!(entry(&lib, &one).is_some() && entry(&lib, &two).is_some(), "nothing stays in _sources");
    assert!(files_below(&lib.root.join("library/materials/_sources")).is_empty());
    let report = migration::report(&Store::for_handle(&lib)).unwrap();
    assert_eq!(report.extra_links, vec![("uid://sbsar_Both".to_string(), two)]);
    assert_eq!(report.substances, 2);
}

#[test]
fn a_replacement_derived_from_a_substance_that_is_not_there_converts_the_archive_first() {
    let base = Base::new("replace");
    let archive = vec![51u8; 6000];
    let lib = step_five(&base, |lib| vec![old_style(lib, "Only", "o.sbsar", &archive)]);
    let source = shared(&archive, "o.sbsar");
    // Step 6 ran, but this archive was skipped (a transient failure, remembered in this state).
    journal_at(&lib, 6);
    let store = Store::for_handle(&lib);
    let meta = std::fs::metadata(lib.root.join(&source)).unwrap();
    let skipped = migration::SkippedFile { path: source.clone(), bytes: Some(meta.len()), modified: Some(meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64), app_version: env!("CARGO_PKG_VERSION").into() };
    let report = migration::SubstanceMigration { skipped: vec![(source.clone(), "busy".into())], skipped_files: vec![skipped], ..Default::default() };
    store.set_meta(migration::META_REPORT, &serde_json::to_string(&report).unwrap()).unwrap();
    store.set_meta(migration::META_DONE, "done").unwrap();
    open(&lib);
    let substance = substance_id(&hash(&archive));
    assert!(entry(&lib, &substance).is_none(), "still skipped");
    // The new plugin re-renders the material: derived from the substance, no archive of its own.
    put(&lib, "library/_import_n/materials/Only/color.png", &png(32, 32, 3));
    let replacement = json!({"id": "uid://sbsar_Only", "kind": "material", "pack": "_import_n", "category": "tiles", "name": "Only", "format": "sbsar",
        "maps": [{"role": "albedo", "file": "color.png"}], "origin": {"plugin": "com.toybox.material-sbsar", "derivedFrom": substance, "meta": {"graph": 0}}});
    merge_entries(&lib, vec![replacement]).unwrap();
    let stored = entry(&lib, &substance).expect("the archive became its substance");
    assert_eq!(std::fs::read(lib.root.join(stored["storagePath"].as_str().unwrap()).join("o.sbsar")).unwrap(), archive);
    assert_eq!(entry(&lib, "uid://sbsar_Only").unwrap()["origin"]["derivedFrom"], json!(substance));
    assert!(!lib.root.join(&source).exists());
    assert_eq!(sbsar_files(&lib).len(), 1);
}

#[test]
fn the_snapshot_before_step_six_is_a_database_the_previous_release_opens() {
    let base = Base::new("snapschema");
    let lib = step_five(&base, |lib| vec![old_style(lib, "A", "s.sbsar", &[7u8; 900])]);
    // This build already upgraded library.db (any open does, before the format steps).
    let live = rusqlite::Connection::open(lib.library_db_path()).unwrap();
    assert_eq!(live.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0)).unwrap(), crate::infra::store::SCHEMA_VERSION);
    drop(live);
    open(&lib);
    let made = crate::infra::snapshot::list(&lib.root);
    assert_eq!(made[0].from_version, 5);
    let connection = rusqlite::Connection::open(&made[0].path).unwrap();
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
    assert_eq!(version, 1, "schema 1: what Toybox 0.5.x reads");
    let column: i64 = connection.query_row("SELECT COUNT(*) FROM pragma_table_info('assets') WHERE name='derived_from'", [], |row| row.get(0)).unwrap();
    assert_eq!(column, 0);
}

#[test]
fn the_substance_folder_is_the_one_a_commit_of_it_would_get() {
    let base = Base::new("samekey");
    let archive = vec![61u8; 2500];
    let lib = step_five(&base, |lib| {
        let mut one = old_style(lib, "One", "k.sbsar", &archive);
        one["tags"] = json!(["  Ceramic", "ceramic", "Tile "]);
        vec![one]
    });
    open(&lib);
    let substance = substance_id(&hash(&archive));
    let stored = entry(&lib, &substance).unwrap();
    let folder = stored["storagePath"].as_str().unwrap().to_string();
    let preview = std::fs::read(lib.root.join(&folder).join("preview.png")).unwrap();
    let mut unstored = stored.clone();
    unstored.as_object_mut().unwrap().remove("storagePath");
    let digests = vec![hash(&archive), hash(&preview)];
    assert_eq!(crate::infra::storage::planned_folder(&unstored, &digests).unwrap(), folder);
    // And `planned_folder` is what a commit computes: stage the same files and plan it.
    let mut staged = unstored.clone();
    staged["pack"] = json!("_import_q");
    // Where an importer stages a plugin-defined asset's files: `<pack>/<type>/<name>/`.
    let name = staged["name"].as_str().unwrap().to_string();
    put(&lib, &format!("library/_import_q/substance/{name}/k.sbsar"), &archive);
    put(&lib, &format!("library/_import_q/substance/{name}/preview.png"), &preview);
    let canonical = lib.root.canonicalize().unwrap();
    let planned = crate::infra::storage::ItemPlan::new(&canonical, vec![staged.clone()]).unwrap().entries()[0]["storagePath"].clone();
    assert_eq!(planned, json!(crate::infra::storage::planned_folder(&staged, &digests).unwrap()));
}

#[test]
fn a_publication_is_refused_while_an_older_journal_waits() {
    let base = Base::new("refuse");
    let lib = library(&base, "a");
    put(&lib, "library/_import_r/materials/M/color.png", &png(8, 8, 1));
    let canonical = lib.root.canonicalize().unwrap();
    let entry = json!({"id": "uid://m", "kind": "material", "pack": "_import_r", "category": "c", "name": "M", "maps": [{"role": "albedo", "file": "color.png"}]});
    let mut plan = crate::infra::storage::ItemPlan::new(&canonical, vec![entry]).unwrap();
    plan.place().unwrap();
    let folder = plan.entries()[0]["storagePath"].as_str().unwrap().to_string();
    put(&lib, ITEMS_JOURNAL, b"{\"older\": true}");
    let error = plan.commit(&lib.cache_path, Default::default(), Delta::default()).unwrap_err();
    assert!(error.to_string().contains("not finished"), "{error}");
    assert_eq!(std::fs::read(lib.root.join(ITEMS_JOURNAL)).unwrap(), b"{\"older\": true}", "not overwritten");
    assert!(!lib.root.join(&folder).exists(), "the refused plan's files are removed");
}
