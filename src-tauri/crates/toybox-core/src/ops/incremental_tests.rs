//! The cost of an operation depends on the items it touches, not on the size of
//! the library. These tests show it two ways: through the rows (a commit stamps
//! the `revision` of the rows it wrote and no others; an item that is already in
//! the library is never read again, so a library with a damaged old item still
//! takes new ones) and by equivalence (a rescan of the library after the
//! operation changes nothing, i.e. the incremental result is the rebuild's).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use rusqlite::Connection;
use serde_json::{json, Value};

use super::catalog::load_catalog_inner;
use super::delete::delete_from_library;
use super::import::merge_entries;
use super::metadata::{update_entries, MetadataEdit};
use super::scan::scan_library_catalog;
use super::thumbs::{list_pending_thumbs, pending_ids, set_thumb_state};
use super::transfer::{export_package, ingest_package, local_reader, ExportOptions, IngestOptions, OnConflict, Outcome};
use crate::domain::catalog_model::ThumbState;
use crate::infra::fsio;
use crate::infra::registry::{LibraryEntry, LibraryHandle};
use crate::infra::store::{self, Delta, Store};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Base(PathBuf);

impl Base {
    fn new(name: &str) -> Base {
        let dir = std::env::temp_dir().join(format!("toybox-incremental-{name}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
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

fn wav(fill: u8) -> Vec<u8> {
    let data = 200u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(8000u32.to_le_bytes());
    bytes.extend(16000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    bytes.extend(vec![fill; data as usize]);
    bytes
}

/// Stage a model whose hierarchy (and so its rig) is named by `rig`, optionally
/// with a texture; return its entry.
fn model(library: &LibraryHandle, pack: &str, id: &str, stem: &str, rig: &str, texture: Option<(&str, &[u8])>) -> Value {
    let mut scene = json!({"asset":{"version":"2.0"},"nodes":[{"name":"Root","children":[1]},{"name":rig}],"buffers":[{"uri":format!("{stem}.bin"),"byteLength":4}]});
    if let Some((name, _)) = texture {
        scene["images"] = json!([{"uri": name}]);
    }
    put(library, &format!("library/{pack}/props/{stem}.gltf"), &serde_json::to_vec(&scene).unwrap());
    put(library, &format!("library/{pack}/props/{stem}.bin"), stem.as_bytes());
    if let Some((name, bytes)) = texture {
        put(library, &format!("library/{pack}/props/{name}"), bytes);
    }
    json!({"id": id, "pack": pack, "category": "props", "file": format!("{stem}.gltf"), "tags": ["wood"], "provenance": {"source": "Pack", "license": "CC0"}})
}

fn sound(library: &LibraryHandle, pack: &str, id: &str, stem: &str, fill: u8) -> Value {
    put(library, &format!("library/{pack}/sfx/{stem}.wav"), &wav(fill));
    json!({"id": id, "kind": "audio", "pack": pack, "category": "sfx", "file": format!("{stem}.wav")})
}

fn db(library: &LibraryHandle) -> Connection {
    Connection::open(library.library_db_path()).unwrap()
}

fn revision(library: &LibraryHandle) -> i64 {
    db(library).query_row("SELECT value FROM meta WHERE key='revision'", [], |row| row.get::<_, String>(0)).unwrap().parse().unwrap()
}

fn strings(library: &LibraryHandle, sql: &str) -> Vec<String> {
    let connection = db(library);
    let mut statement = connection.prepare(sql).unwrap();
    let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap();
    rows.map(Result::unwrap).collect()
}

fn count(library: &LibraryHandle, sql: &str) -> i64 {
    db(library).query_row(sql, [], |row| row.get(0)).unwrap()
}

/// Ids of the rows of `table` written by revision `rev`.
fn stamped(library: &LibraryHandle, table: &str, rev: i64) -> Vec<String> {
    strings(library, &format!("SELECT id FROM {table} WHERE revision = {rev} ORDER BY id"))
}

/// The saved catalog as a document that does not depend on row order or on the
/// order rigs were first met.
fn saved(library: &LibraryHandle) -> Value {
    let mut doc = serde_json::to_value(Store::for_handle(library).load_catalog().unwrap().expect("a catalog")).unwrap();
    doc["libraryRoot"] = json!("");
    let sort = |list: &mut Value, key: &str| {
        if let Some(items) = list.as_array_mut() {
            items.sort_by_key(|item| item[key].as_str().unwrap_or("").to_string());
        }
    };
    sort(&mut doc["assets"], "id");
    sort(&mut doc["rigs"], "id");
    doc
}

/// The incremental result is the rebuild's: a rescan finds nothing to change in
/// the manifest or in the catalog.
fn assert_a_rescan_changes_nothing(library: &LibraryHandle) {
    let manifest = store::read_manifest(&library.root).unwrap();
    let before = saved(library);
    scan_library_catalog(library).unwrap();
    assert_eq!(store::read_manifest(&library.root).unwrap(), manifest, "the manifest is what a rescan reads");
    assert_eq!(saved(library), before, "the catalog is what a rescan builds");
}

fn ids_of(library: &LibraryHandle, table: &str) -> Vec<String> {
    strings(library, &format!("SELECT id FROM {table} ORDER BY id"))
}

fn edit(id: &str, tags: &[&str]) -> MetadataEdit {
    MetadataEdit { id: id.into(), tags: Some(tags.iter().map(|t| t.to_string()).collect()), provenance: None }
}

/// A library with five items: three models (the first two share a texture and a
/// rig), a model with a rig of its own, and a sound.
fn five(base: &Base) -> LibraryHandle {
    let library = library(base, "lib");
    let tex: &[u8] = b"shared texture";
    let entries = vec![
        model(&library, "_import_a", "uid://m1", "M1", "PartA", Some(("wood.png", tex))),
        model(&library, "_import_a", "uid://m2", "M2", "PartA", Some(("wood.png", tex))),
        model(&library, "_import_a", "uid://m3", "M3", "PartB", None),
        sound(&library, "_import_a", "uid://s1", "S1", 1),
    ];
    merge_entries(&library, entries).unwrap();
    merge_entries(&library, vec![model(&library, "_import_b", "uid://m4", "M4", "PartC", None)]).unwrap();
    library
}

#[test]
fn an_import_writes_only_the_new_rows_and_never_reads_an_item_that_is_there() {
    let base = Base::new("import");
    let library = five(&base);
    let before = revision(&library);
    let items_before = ids_of(&library, "items");
    // An item that is already stored is damaged on disk: a rebuild of the library would fail on it.
    let stored = strings(&library, "SELECT storage_path FROM items WHERE id='uid://m3'").remove(0);
    std::fs::remove_file(library.root.join(&stored)).unwrap();

    let merged = merge_entries(&library, vec![model(&library, "_import_c", "uid://new", "New", "PartD", None)]).unwrap();
    assert_eq!(merged.assets.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["uid://new"]);
    assert!(merged.removed.is_empty());

    assert_eq!(revision(&library), before + 1, "one revision for the commit");
    assert_eq!(stamped(&library, "items", before + 1), ["uid://new"]);
    assert_eq!(stamped(&library, "assets", before + 1), ["uid://new"]);
    assert_eq!(strings(&library, &format!("SELECT asset_id FROM changes WHERE revision = {} ORDER BY asset_id", before + 1)), ["uid://new"]);
    assert_eq!(ids_of(&library, "items").len(), items_before.len() + 1);
    // Its file references, rig use, variants table and hash arrived with it.
    assert!(count(&library, "SELECT COUNT(*) FROM asset_files WHERE asset_id='uid://new'") >= 2);
    assert_eq!(count(&library, "SELECT COUNT(*) FROM rig_uses WHERE asset_id='uid://new'"), 1);
    assert_eq!(count(&library, "SELECT COUNT(*) FROM assets WHERE id='uid://new' AND content_hash IS NOT NULL"), 1);
    assert_eq!(count(&library, "SELECT COUNT(*) FROM rigs"), 4, "PartA, PartB, PartC and the new PartD");
    // Nothing of the staging is left behind.
    assert!(!library.root.join("library/_import_c").exists());
    let backups = library.root.join("_library_config/migration-backups");
    let journal = std::fs::read_dir(&backups).unwrap().flatten().map(|e| e.path().join("items.before.json")).filter(|p| p.is_file()).max().unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(journal).unwrap()).unwrap(), json!({}), "a new item had no entry before");
    assert!(!library.root.join("_library_config/storage-pending-items.json").exists());
}

#[test]
fn an_import_equals_a_rebuild() {
    let base = Base::new("import-equal");
    let library = five(&base);
    assert_a_rescan_changes_nothing(&library);
    // A replaced item keeps what the library knew of it.
    let mut again = model(&library, "_import_r", "uid://m1", "M1b", "PartA", None);
    again.as_object_mut().unwrap().remove("tags");
    again.as_object_mut().unwrap().remove("provenance");
    let mut cached = load_catalog_inner(&library).unwrap().unwrap();
    for asset in &mut cached.assets {
        if asset.id == "uid://m1" {
            asset.user.favorite = true;
            asset.user.collections.push("Keep".into());
        }
    }
    super::catalog::save_catalog_inner(&library, &cached).unwrap();
    let before = revision(&library);
    let merged = merge_entries(&library, vec![again.clone()]).unwrap();
    assert!(merged.assets[0].user.favorite && merged.assets[0].user.collections == ["Keep"]);
    assert_eq!(stamped(&library, "items", before + 1), ["uid://m1"]);
    assert_eq!(stamped(&library, "assets", before + 1), ["uid://m1"]);
    let entry = &store::read_manifest(&library.root).unwrap()["assets"][0];
    assert_eq!(entry["id"], "uid://m1", "a replaced item stays where it was");
    assert_eq!(entry["tags"], json!(["wood"]), "and keeps its tags and provenance");
    assert_eq!(entry["provenance"]["source"], "Pack");
    assert_a_rescan_changes_nothing(&library);
    // The same import again changes nothing at all.
    let before = revision(&library);
    model(&library, "_import_r", "uid://m1", "M1b", "PartA", None);
    merge_entries(&library, vec![again]).unwrap();
    assert_eq!(revision(&library), before);
}

#[test]
fn a_variant_replace_absorbs_the_rows_it_groups_and_equals_a_rebuild() {
    let base = Base::new("variants");
    let library = library(&base, "lib");
    merge_entries(&library, vec![
        model(&library, "_import_v", "old-full", "Tree_LOD0", "Trunk", None),
        model(&library, "_import_v", "old-low", "Tree_LOD2", "Trunk", None),
        model(&library, "_import_v", "other", "Rock", "Stone", None),
    ])
    .unwrap();
    let mut cached = load_catalog_inner(&library).unwrap().unwrap();
    for asset in &mut cached.assets {
        if asset.id == "old-low" {
            asset.user.favorite = true;
            asset.user.tags.push("mine".into());
        }
    }
    super::catalog::save_catalog_inner(&library, &cached).unwrap();
    assert_a_rescan_changes_nothing(&library);

    let before = revision(&library);
    // The stored rows are in the pack "imported"; an entry for the same physical files names it too.
    let mut grouped = model(&library, "imported", "tree", "Tree_LOD0", "Trunk", None);
    model(&library, "imported", "old-low", "Tree_LOD2", "Trunk", None);
    grouped["variants"] = json!([{"id": "old-low", "pack": "imported", "category": "props", "file": "Tree_LOD2.gltf"}]);
    let merged = merge_entries(&library, vec![grouped]).unwrap();
    let mut removed = merged.removed.clone();
    removed.sort();
    assert_eq!(removed, ["old-full", "old-low"], "the rows with the same physical files are absorbed");
    assert_eq!(ids_of(&library, "items"), ["other", "tree"]);
    assert_eq!(ids_of(&library, "assets"), ["other", "tree"]);
    assert_eq!(stamped(&library, "items", before + 1), ["tree"]);
    assert_eq!(strings(&library, "SELECT variant_id FROM item_variants WHERE item_id='tree'"), ["old-low"]);
    let tree = &merged.assets[0];
    assert!(tree.user.favorite && tree.user.tags.contains(&"mine".to_string()), "what the library knew of the absorbed row moves to the group");
    assert_eq!(count(&library, "SELECT COUNT(*) FROM asset_files WHERE asset_id='tree'"), 4, "the files of both sizes");
    assert_a_rescan_changes_nothing(&library);
}

/// A model with one animation and a rest pose, as `animation_import` expects it.
fn animated(library: &LibraryHandle, pack: &str, stem: &str) {
    let times: Vec<u8> = [0.0f32, 1.0].into_iter().flat_map(f32::to_le_bytes).collect();
    let values: Vec<u8> = [0.0f32, 0.0, 0.0, 2.0, 0.0, 0.0].into_iter().flat_map(f32::to_le_bytes).collect();
    let bytes = [times, values].concat();
    let doc = json!({"asset":{"version":"2.0"},"nodes":[{"name":"Root","mesh":0}],"meshes":[{"primitives":[]}],"buffers":[{"uri":format!("{stem}.bin"),"byteLength":32}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":8},{"buffer":0,"byteOffset":8,"byteLength":24}],
        "accessors":[{"bufferView":0,"componentType":5126,"type":"SCALAR","count":2,"min":[0],"max":[1]},{"bufferView":1,"componentType":5126,"type":"VEC3","count":2}],
        "animations":[{"name":"Empty","channels":[]},{"name":"Walk","samplers":[{"input":0,"output":1}],"channels":[{"sampler":0,"target":{"node":0,"path":"translation"}}]}]});
    put(library, &format!("library/{pack}/chars/{stem}.gltf"), &serde_json::to_vec(&doc).unwrap());
    put(library, &format!("library/{pack}/chars/{stem}.bin"), &bytes);
}

#[test]
fn an_animation_source_becomes_clips_by_the_same_rows_a_rebuild_makes() {
    let base = Base::new("clips");
    let library = five(&base);
    animated(&library, "_import_h", "Hero");
    let before = revision(&library);
    let merged = merge_entries(&library, vec![json!({"id": "hero", "pack": "_import_h", "category": "chars", "file": "Hero.gltf", "provenance": {"source": "Locomotion"}})]).unwrap();
    // The model and the clip cut from it are two items, each with its own row.
    let stamped_items = stamped(&library, "items", before + 1);
    assert_eq!(stamped_items.len(), 2, "{stamped_items:?}");
    assert_eq!(stamped(&library, "assets", before + 1), stamped_items);
    assert_eq!(merged.assets.len(), 2);
    let clip = merged.assets.iter().find(|a| matches!(a.content, crate::domain::catalog_model::AssetContent::Animation { .. })).unwrap();
    assert_eq!(count(&library, &format!("SELECT COUNT(*) FROM assets WHERE clip_source='hero' AND id='{}'", clip.id)), 1);
    assert_eq!(count(&library, &format!("SELECT COUNT(*) FROM assets WHERE id='{}' AND item_id='{}'", clip.id, clip.id)), 1);
    assert_a_rescan_changes_nothing(&library);

    // An animation-only file is replaced by its clips: no item of its own.
    animated(&library, "_import_w", "Wave");
    let before = revision(&library);
    let merged = merge_entries(&library, vec![json!({"kind": "animation", "id": "wave-src", "pack": "_import_w", "category": "chars", "file": "Wave.gltf"})]).unwrap();
    assert_eq!(merged.assets.len(), 1);
    assert_eq!(count(&library, "SELECT COUNT(*) FROM items WHERE id='wave-src'"), 0);
    assert_eq!(stamped(&library, "items", before + 1).len(), 1);
    assert_a_rescan_changes_nothing(&library);
    // Cut again, the clips are the same rows: nothing is written.
    let before = revision(&library);
    animated(&library, "_import_w", "Wave");
    merge_entries(&library, vec![json!({"kind": "animation", "id": "wave-src", "pack": "_import_w", "category": "chars", "file": "Wave.gltf"})]).unwrap();
    assert_eq!(revision(&library), before);
}

#[test]
fn a_metadata_edit_writes_only_the_edited_rows_and_equals_a_rebuild() {
    let base = Base::new("metadata");
    let library = five(&base);
    let before = revision(&library);
    let edited = update_entries(&library, &[edit("uid://m3", &["Iron", "heavy"])]).unwrap();
    assert_eq!(edited.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["uid://m3"]);
    assert_eq!(edited[0].tags, ["iron", "heavy"]);
    assert_eq!(revision(&library), before + 1);
    assert_eq!(stamped(&library, "items", before + 1), ["uid://m3"]);
    assert_eq!(stamped(&library, "assets", before + 1), ["uid://m3"]);
    assert_eq!(strings(&library, "SELECT tag FROM asset_tags WHERE asset_id='uid://m3' ORDER BY tag"), ["heavy", "iron"]);
    assert_eq!(count(&library, "SELECT COUNT(*) FROM asset_search WHERE asset_search MATCH 'iro'"), 1);
    // The safety copy holds the edited entry as it was, and nothing else.
    let copy = std::fs::read_dir(library.root.join("_library_config")).unwrap().flatten().map(|e| e.path()).find(|p| p.file_name().unwrap().to_string_lossy().starts_with("catalog-before-metadata-")).unwrap();
    let copy: Value = serde_json::from_slice(&std::fs::read(copy).unwrap()).unwrap();
    assert_eq!(copy["assets"].as_array().unwrap().len(), 1);
    assert_eq!(copy["assets"][0]["id"], "uid://m3");
    assert_eq!(copy["assets"][0]["tags"], json!(["wood"]));
    assert_a_rescan_changes_nothing(&library);

    // All or nothing: one unknown id refuses the whole request.
    let before = revision(&library);
    assert!(update_entries(&library, &[edit("uid://m1", &["x"]), edit("uid://nope", &["y"])]).is_err());
    assert_eq!(revision(&library), before);
    assert!(update_entries(&library, &[]).is_err());
    let too_many: Vec<MetadataEdit> = (0..=super::metadata::MAX_EDITS).map(|n| edit(&format!("uid://m{n}"), &["x"])).collect();
    assert!(update_entries(&library, &too_many).is_err(), "the cap stays");
    // Stored files are never read for an edit.
    let stored = strings(&library, "SELECT storage_path FROM items WHERE id='uid://m4'").remove(0);
    std::fs::remove_file(library.root.join(stored)).unwrap();
    update_entries(&library, &[edit("uid://s1", &["ping"])]).unwrap();
}

#[test]
fn a_delete_removes_only_the_deleted_rows_and_what_only_they_own() {
    let base = Base::new("delete");
    let library = five(&base);
    let store = Store::for_handle(&library);
    let m1_files = store.exclusive_files(&["uid://m1".to_string()]).unwrap();
    assert_eq!(m1_files.len(), 2, "its scene and bin; the texture and rig are shared with m2");
    // The ownership questions have the answers the catalog walk gave.
    for id in ["uid://m1", "uid://m2", "uid://m3"] {
        fsio::write_bytes(&library.thumb_path(id), b"png").unwrap();
    }
    let catalog = load_catalog_inner(&library).unwrap().unwrap();
    for ids in [vec!["uid://m1"], vec!["uid://m1", "uid://m2"], vec!["uid://s1", "uid://m4"]] {
        let ids: Vec<String> = ids.into_iter().map(String::from).collect();
        assert_eq!(super::delete_files::exclusive_files_in(&library.root, &store, &ids).unwrap(), super::delete_files::exclusive_files(&library.root, &catalog, &ids).unwrap());
        assert_eq!(
            super::delete_files::exclusive_thumbnails_in(&library.cache_path, &store, &ids).unwrap(),
            super::delete_files::exclusive_thumbnails(&library.cache_path, &catalog, &ids).unwrap()
        );
    }
    let texture = strings(&library, "SELECT path FROM asset_files WHERE asset_id='uid://m1' AND path LIKE '%_resources%'").remove(0);

    let before = revision(&library);
    let removed = delete_from_library(&library.root, &library.cache_path, &["uid://m1".to_string()]).unwrap();
    assert_eq!(removed, ["uid://m1"]);
    assert_eq!(revision(&library), before + 1);
    assert_eq!(ids_of(&library, "items"), ["uid://m2", "uid://m3", "uid://m4", "uid://s1"]);
    assert_eq!(ids_of(&library, "assets"), ["uid://m2", "uid://m3", "uid://m4", "uid://s1"]);
    assert_eq!(strings(&library, &format!("SELECT asset_id FROM changes WHERE revision = {before} + 1")), ["uid://m1"]);
    assert!(library.root.join(&texture).exists(), "a file another item uses stays");
    assert_eq!(count(&library, "SELECT COUNT(*) FROM rigs WHERE id IN (SELECT rig_id FROM rig_uses WHERE asset_id='uid://m2')"), 1, "the rig it shared stays");
    assert_eq!(count(&library, "SELECT COUNT(*) FROM rigs"), 3);
    assert!(!library.thumb_path("uid://m1").exists() && library.thumb_path("uid://m2").exists() && library.thumb_path("uid://m3").exists());
    // The safety copies hold the deleted item only.
    let copies = |dir: &Path| -> Vec<Value> {
        std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("catalog-before-delete-")).map(|p| serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()).collect()
    };
    let in_library = copies(&library.root.join("_library_config"));
    assert_eq!(in_library.len(), 1);
    assert_eq!(in_library[0]["assets"].as_array().unwrap().iter().map(|e| e["id"].clone()).collect::<Vec<_>>(), [json!("uid://m1")]);
    let beside_cache = copies(library.cache_path.parent().unwrap());
    assert_eq!(beside_cache[0]["assets"].as_array().unwrap().len(), 1);
    assert_a_rescan_changes_nothing(&library);

    // Its last user going takes the shared texture and the rig with it.
    delete_from_library(&library.root, &library.cache_path, &["uid://m2".to_string()]).unwrap();
    assert!(!library.root.join(&texture).exists());
    assert_eq!(count(&library, "SELECT COUNT(*) FROM rigs"), 2, "PartA lost its last user");
    assert_eq!(count(&library, "SELECT COUNT(*) FROM rig_uses WHERE asset_id NOT IN (SELECT id FROM assets)"), 0);
    assert_eq!(count(&library, "SELECT COUNT(*) FROM asset_files WHERE asset_id NOT IN (SELECT id FROM assets)"), 0);
    assert_a_rescan_changes_nothing(&library);
    assert!(delete_from_library(&library.root, &library.cache_path, &["uid://gone".to_string()]).is_err());
}

#[test]
fn deleting_clears_the_preview_choices_that_named_the_deleted_item() {
    let base = Base::new("previews");
    let library = five(&base);
    let mut cached = load_catalog_inner(&library).unwrap().unwrap();
    for asset in &mut cached.assets {
        if asset.id == "uid://m2" {
            asset.user.preview_asset_id = Some("uid://m1".into());
        }
    }
    super::catalog::save_catalog_inner(&library, &cached).unwrap();
    delete_from_library(&library.root, &library.cache_path, &["uid://m1".to_string()]).unwrap();
    let after = Store::for_handle(&library).load_top_assets(&["uid://m2".to_string()]).unwrap();
    assert!(after[0].user.preview_asset_id.is_none());
    let user = Connection::open(library.user_db_path()).unwrap();
    assert_eq!(user.query_row::<i64, _, _>("SELECT COUNT(*) FROM user_assets WHERE asset_id='uid://m2'", [], |r| r.get(0)).unwrap(), 0, "a row that is all defaults is not kept");
}

#[test]
fn an_ingest_with_conflicts_writes_only_the_items_it_lands_and_equals_a_rebuild() {
    let base = Base::new("ingest");
    let source = library(&base, "src");
    merge_entries(&source, vec![
        model(&source, "_import_s", "uid://a", "A", "PartA", Some(("tex.png", b"tex"))),
        model(&source, "_import_s", "uid://b", "B", "PartB", None),
        sound(&source, "_import_s", "uid://c", "C", 3),
    ])
    .unwrap();
    let target = library(&base, "dst");
    merge_entries(&target, vec![model(&target, "_import_t", "uid://a", "A", "PartA", None), model(&target, "_import_t", "uid://z", "Z", "PartZ", None)]).unwrap();
    let ids: Vec<String> = ["uid://a", "uid://b", "uid://c"].into_iter().map(String::from).collect();
    let package = export_package(&source, &ids, ExportOptions::default()).unwrap();
    let reader = local_reader(&source);

    let before = revision(&target);
    let report = ingest_package(&target, &package, IngestOptions { on_conflict: OnConflict::Skip }, &reader, &|_| {}).unwrap();
    let outcome = |report: &super::transfer::TransferReport, id: &str| report.items.iter().find(|i| i.id == id).unwrap().outcome;
    assert_eq!((outcome(&report, "uid://a"), outcome(&report, "uid://b"), outcome(&report, "uid://c")), (Outcome::Skipped, Outcome::Copied, Outcome::Copied));
    let landed: Vec<String> = ["uid://b", "uid://c"].into_iter().map(String::from).collect();
    let mut written = stamped(&target, "items", before + 1);
    written.extend(stamped(&target, "items", before + 2));
    written.sort();
    written.dedup();
    assert_eq!(written, landed, "the skipped item and the one that was there are not written");
    assert_eq!(ids_of(&target, "items"), ["uid://a", "uid://b", "uid://c", "uid://z"]);
    assert_a_rescan_changes_nothing(&target);

    // Replace: only the replaced item and nothing else changes, and the target keeps its own favourites.
    let mut cached = load_catalog_inner(&target).unwrap().unwrap();
    for asset in &mut cached.assets {
        if asset.id == "uid://a" {
            asset.user.favorite = true;
        }
    }
    super::catalog::save_catalog_inner(&target, &cached).unwrap();
    update_entries(&source, &[edit("uid://a", &["from-source"])]).unwrap();
    let package = export_package(&source, &["uid://a".to_string()], ExportOptions::default()).unwrap();
    let before = revision(&target);
    let report = ingest_package(&target, &package, IngestOptions { on_conflict: OnConflict::Replace }, &reader, &|_| {}).unwrap();
    assert_eq!(outcome(&report, "uid://a"), Outcome::Replaced);
    let after: Vec<String> = (before + 1..=revision(&target)).flat_map(|rev| stamped(&target, "assets", rev)).collect();
    assert_eq!(after, ["uid://a"]);
    let a = &Store::for_handle(&target).load_top_assets(&["uid://a".to_string()]).unwrap()[0];
    assert_eq!(a.tags, ["from-source"]);
    assert!(a.user.favorite);
    assert_a_rescan_changes_nothing(&target);
}

#[test]
fn a_thumbnail_state_is_one_personal_row_and_pending_is_a_stream() {
    let base = Base::new("thumbs");
    let library = five(&base);
    let before = revision(&library);
    set_thumb_state(&library, "uid://m3", ThumbState::Error, Some("no webgl".into())).unwrap();
    assert_eq!(revision(&library), before, "a thumbnail state is not a library change");
    let user = Connection::open(library.user_db_path()).unwrap();
    assert_eq!(user.query_row::<i64, _, _>("SELECT COUNT(*) FROM user_assets WHERE thumb_error IS NOT NULL", [], |r| r.get(0)).unwrap(), 1);
    let loaded = load_catalog_inner(&library).unwrap().unwrap();
    let m3 = loaded.assets.iter().find(|a| a.id == "uid://m3").unwrap();
    assert!(matches!(m3.thumb.state, ThumbState::Error) && m3.thumb.error.as_deref() == Some("no webgl"));
    set_thumb_state(&library, "uid://m3", ThumbState::Missing, None).unwrap();
    assert_eq!(user.query_row::<i64, _, _>("SELECT COUNT(*) FROM user_assets", [], |r| r.get(0)).unwrap(), 0);
    set_thumb_state(&library, "uid://not-there", ThumbState::Error, None).unwrap();

    fsio::write_bytes(&library.thumb_path("uid://m1"), b"png").unwrap();
    fsio::write_bytes(&library.thumb_path("uid://m2"), b"png").unwrap();
    fsio::write_bytes(&library.thumb_path("uid://m2").with_extension("stale"), b"").unwrap();
    let streamed = list_pending_thumbs(&library).unwrap();
    assert_eq!(streamed, pending_ids(&library, &load_catalog_inner(&library).unwrap().unwrap()));
    assert!(!streamed.contains(&"uid://m1".to_string()) && streamed.contains(&"uid://m2".to_string()));
}

#[test]
fn applying_the_same_delta_twice_writes_nothing_the_second_time() {
    let base = Base::new("idempotent");
    let library = five(&base);
    let store = Store::for_handle(&library);
    let assets = store.load_top_assets(&["uid://m1".to_string(), "uid://s1".to_string()]).unwrap();
    let entries = store.load_entries(&["uid://m1".to_string(), "uid://s1".to_string()]).unwrap();
    let puts = assets.iter().zip(entries).map(|(asset, entry)| store::ItemPut { id: asset.id.clone(), entry: Some(entry), assets: vec![asset.clone()], rigs: Vec::new() }).collect();
    let before = revision(&library);
    let applied = store.apply(&Delta { puts, ..Default::default() }).unwrap();
    assert!(!applied.committed.changed);
    assert_eq!(revision(&library), before);
    // A removal of what is not there is no change either.
    let applied = store.apply(&Delta { remove_items: vec!["uid://nope".into()], ..Default::default() }).unwrap();
    assert!(!applied.committed.changed);
}

#[test]
fn single_rows_are_read_by_id_whether_they_are_items_or_variants() {
    let base = Base::new("by-id");
    let library = library(&base, "lib");
    let mut grouped = model(&library, "_import_g", "tree", "Tree_LOD0", "Trunk", None);
    model(&library, "_import_g", "low", "Tree_LOD2", "Trunk", None);
    grouped["variants"] = json!([{"id": "low", "pack": "_import_g", "category": "props", "file": "Tree_LOD2.gltf"}]);
    merge_entries(&library, vec![grouped, sound(&library, "_import_g", "uid://s", "S", 9)]).unwrap();
    let store = Store::for_handle(&library);
    let found = store.load_assets(&["uid://s".to_string(), "low".to_string(), "missing".to_string(), "tree".to_string()]).unwrap();
    assert_eq!(found.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["uid://s", "low", "tree"], "in the order asked, the missing one left out");
    assert_eq!(store.id_owners(&["low".to_string(), "tree".to_string(), "x".to_string()]).unwrap().get("low").map(String::as_str), Some("tree"));
    assert_eq!(store.present_items(&["low".to_string(), "tree".to_string()]).unwrap().into_iter().collect::<Vec<_>>(), ["tree"]);
    let rigs = store.rigs_for(&found).unwrap();
    assert_eq!(rigs.len(), 1, "the rigs of the asked assets only");
}

#[test]
fn the_personal_tag_migration_is_recorded_and_not_repeated() {
    let base = Base::new("personal");
    let library = five(&base);
    let mut cached = load_catalog_inner(&library).unwrap().unwrap();
    for asset in &mut cached.assets {
        if asset.id == "uid://m3" {
            asset.user.tags.push("old personal".into());
        }
    }
    super::catalog::save_catalog_inner(&library, &cached).unwrap();
    let loaded = super::metadata::load_catalog_migrating(&library).unwrap().unwrap();
    let m3 = loaded.assets.iter().find(|a| a.id == "uid://m3").unwrap();
    assert!(m3.tags.contains(&"old personal".to_string()) && !m3.user.tags.contains(&"old personal".to_string()));
    let store = Store::for_handle(&library);
    // The migration ran its edit; the next load finds the library settled and does no tag work.
    super::metadata::load_catalog_migrating(&library).unwrap();
    assert_eq!(store.meta("personalTagsMigrated").as_deref(), Some("1"));
    let before = revision(&library);
    super::metadata::load_catalog_migrating(&library).unwrap();
    assert_eq!(revision(&library), before);
}

#[test]
fn a_refused_import_changes_nothing() {
    let base = Base::new("rollback");
    let library = five(&base);
    // The second entry is unreadable audio: validation refuses the batch before anything is placed.
    put(&library, "library/_import_x/sfx/Bad.wav", b"not audio");
    let bad = json!({"id": "uid://bad", "kind": "audio", "pack": "_import_x", "category": "sfx", "file": "Bad.wav"});
    let good = model(&library, "_import_x", "uid://good", "Good", "PartG", None);
    let before = (revision(&library), ids_of(&library, "items"), std::fs::read_dir(library.root.join("library/models")).unwrap().count());
    assert!(merge_entries(&library, vec![good, bad]).is_err());
    assert_eq!((revision(&library), ids_of(&library, "items"), std::fs::read_dir(library.root.join("library/models")).unwrap().count()), before);
}

#[test]
fn hashes_are_found_and_saved_by_key_for_the_items_asked_about() {
    use super::content_hash::{fill_assets, fill_from_cache, hash_new, storage_key};
    let base = Base::new("hashes");
    let library = five(&base);
    let store = Store::for_handle(&library);
    let ids = ["uid://m1".to_string(), "uid://m2".to_string()];
    let mut asked = store.load_top_assets(&ids).unwrap();
    let key = storage_key(&asked[0]).unwrap();
    store::hashes::update_keys(&library.library_db_path(), &[], &[key.clone()]).unwrap();
    let hash_revision = || store.stamp().unwrap().hash_revision;
    let before = hash_revision();
    assert_eq!(count(&library, "SELECT COUNT(*) FROM assets WHERE id='uid://m1' AND content_hash IS NULL"), 1, "the row stops showing it too");
    assert_eq!(hash_new(&library, &mut asked), 1, "only the item that lacked one is read");
    assert_eq!(hash_revision(), before + 1, "one revision for the hash that came back");
    assert!(asked.iter().all(|asset| asset.content_hash.is_some()));
    assert_eq!(count(&library, "SELECT COUNT(*) FROM assets WHERE id='uid://m1' AND content_hash IS NOT NULL"), 1);
    // The same values the whole-catalog fill gives.
    let mut whole = load_catalog_inner(&library).unwrap().unwrap();
    fill_from_cache(&library, &mut whole);
    let mut by_key = store.load_top_assets(&ids).unwrap();
    fill_assets(&library, &mut by_key);
    for asset in &by_key {
        assert_eq!(asset.content_hash, whole.assets.iter().find(|a| a.id == asset.id).unwrap().content_hash);
    }
    assert_eq!(hash_new(&library, &mut by_key), 0);
    assert_eq!(hash_revision(), before + 1, "nothing new, nothing written");
}
