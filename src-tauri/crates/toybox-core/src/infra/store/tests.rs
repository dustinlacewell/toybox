use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::{json, Value};

use super::*;
use crate::domain::catalog_model::{Asset, Catalog, ThumbMeta, ThumbState};

pub(crate) struct Tmp(pub PathBuf);
impl Tmp {
    pub fn new(tag: &str) -> Tmp {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("toybox-store-{tag}-{}-{}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }
    pub fn root(&self) -> PathBuf { self.0.join("library") }
    pub fn cache(&self) -> PathBuf { self.0.join("appdata/catalog.json") }
    pub fn store(&self) -> Store { Store::local(&self.root(), &self.cache()) }
}
impl Drop for Tmp {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

fn key(c: char) -> String { c.to_string().repeat(64) }

/// A manifest with an unknown top-level key, unknown keys inside entries and a
/// non-alphabetical key order everywhere.
pub(crate) fn manifest_doc() -> Value {
    json!({
        "zebra": {"keep": [1, 2, 3]},
        "schemaVersion": 1,
        "storageVersion": 1,
        "assets": [
            {"id": "uid://model", "kind": "model", "pack": "p", "category": "props", "file": "Crate.gltf", "storagePath": format!("library/models/{}/Crate.gltf", key('a')), "tags": ["wood", "box"], "provenance": {"source": "Pack", "license": "CC0"}, "vendorExtension": {"b": 1, "a": 2}},
            {"id": "uid://mat", "kind": "material", "pack": "p", "category": "stone", "name": "Stone", "storagePath": format!("library/materials/{}", key('b')), "maps": [{"role": "albedo", "file": "a.png"}]},
            {"id": "uid://audio", "kind": "audio", "pack": "p", "category": "sfx", "file": "Boom.wav", "storagePath": format!("library/audio/{}/Boom.wav", key('c'))},
            {"id": "uid://sky", "kind": "plugin", "type": "skybox", "pack": "p", "category": "sky", "name": "Dusk", "files": ["a.hdr"], "meta": {"exposure": 1.5}, "storagePath": format!("library/skybox/{}", key('d'))},
            {"id": "uid://clip", "kind": "animation", "pack": "p", "category": "anim", "file": "Walk.gltf", "storagePath": format!("library/animations/{}/Walk.gltf", key('e'))}
        ],
        "alpha": "last-but-one"
    })
}

fn asset(value: Value) -> Asset { serde_json::from_value(value).unwrap() }

fn user(favorite: bool, tags: &[&str], collections: &[&str]) -> Value {
    json!({"favorite": favorite, "tags": tags, "collections": collections})
}

/// One asset of every kind, with personal data, variants, a clip, peaks and plugin meta.
pub(crate) fn catalog() -> Catalog {
    let model = asset(json!({
        "id": "uid://model", "name": "Crate", "fileName": "Crate.gltf", "relPath": format!("library/models/{}/Crate.gltf", key('a')), "pack": "p", "category": "props",
        "kind": "model", "fileset": {"gltf": format!("library/models/{}/Crate.gltf", key('a')), "bin": format!("library/models/{}/Crate.bin", key('a')), "textures": ["library/models/_resources/t.png"]},
        "animation": {"clipCount": 0, "clipNames": []}, "format": "fbx", "tags": ["box", "wood"],
        "origin": {"plugin": "fbx-importer", "meta": {"scale": 0.01}, "files": ["Crate.fbx"]},
        "thumb": {"state": "error", "error": "render failed"},
        "user": {"provenance": {"filesystemPath": "D:/downloads/Pack", "source": "Pack", "license": "CC0"}, "previewAssetId": "uid://model-low", "favorite": true, "tags": ["source: Pack", "license: CC0", "mine", "also mine"], "collections": ["Zoo", "Alpha"]},
        "rigs": [{"rigId": "humanoid", "nodes": {"Hips": 0}}],
        "variants": [{
            "id": "uid://model-low", "name": "Crate_LOD1", "fileName": "Crate_LOD1.gltf", "relPath": format!("library/models/{}/Crate_LOD1.gltf", key('f')), "pack": "p", "category": "props",
            "kind": "model", "fileset": {"gltf": format!("library/models/{}/Crate_LOD1.gltf", key('f')), "bin": format!("library/models/{}/Crate_LOD1.bin", key('f')), "textures": []},
            "thumb": {"state": "missing"}, "user": user(true, &[], &["Low"])
        }]
    }));
    let material = asset(json!({
        "id": "uid://mat", "name": "Stone", "fileName": "Stone", "relPath": format!("library/materials/{}", key('b')), "pack": "p", "category": "stone",
        "kind": "material", "material": {"maps": [{"role": "albedo", "file": format!("library/materials/{}/a.png", key('b')), "invert": false}, {"role": "roughness", "file": format!("library/materials/{}/a.png", key('b')), "channel": "g", "invert": true}], "flipNormalGreen": true},
        "thumb": {"state": "missing"}, "user": user(false, &["rock"], &[])
    }));
    let audio = asset(json!({
        "id": "uid://audio", "name": "Boom", "fileName": "Boom.wav", "relPath": format!("library/audio/{}/Boom.wav", key('c')), "pack": "p", "category": "sfx",
        "kind": "audio", "audio": {"file": format!("library/audio/{}/Boom.wav", key('c')), "format": "wav", "duration": 1.25, "sampleRate": 48000, "channels": 2, "bitDepth": 16, "byteLength": 480000, "modifiedNs": "1700000000000000000", "peaks": [0.0, 0.5, 1.0, 0.25]},
        "thumb": {"state": "missing"}, "user": user(false, &[], &[])
    }));
    let plugin = asset(json!({
        "id": "uid://sky", "name": "Dusk", "fileName": "Dusk", "relPath": format!("library/skybox/{}", key('d')), "pack": "p", "category": "sky",
        "kind": "plugin", "type": "skybox", "files": [format!("library/skybox/{}/a.hdr", key('d'))], "meta": {"exposure": 1.5, "nested": {"z": 1, "a": [1, 2]}},
        "thumb": {"state": "missing"}, "user": user(false, &["sunset"], &["Skies"])
    }));
    let clip = asset(json!({
        "id": "clip:walk", "name": "Walk", "fileName": "Walk.gltf", "relPath": format!("library/animations/{}/Walk.gltf", key('e')), "pack": "p", "category": "anim",
        "kind": "animation", "fileset": {"gltf": format!("library/animations/{}/Walk.gltf", key('e')), "bin": format!("library/animations/{}/Walk.bin", key('e')), "textures": []},
        "clip": {"tracks": [{"node": 1, "path": "translation"}], "standalone": false, "index": 2, "name": "Walk", "duration": 1.0, "sourceId": "uid://clip", "sourceName": "Hero", "targetNodes": ["Root"], "rig": "Biped · 20 nodes"},
        "thumb": {"state": "missing"}, "user": user(false, &[], &[])
    }));
    Catalog {
        rigs: serde_json::from_value(json!([{"id": "humanoid", "name": "Humanoid", "kind": "humanoid", "unambiguous": true, "nodes": []}, {"id": "quad", "name": "Quad", "kind": "other", "unambiguous": false, "nodes": []}])).unwrap(),
        schema_version: crate::config::SCHEMA_VERSION,
        library_root: String::new(),
        assets: vec![model, material, audio, plugin, clip],
    }
}

pub(crate) fn rooted(mut catalog: Catalog, store: &Store) -> Catalog {
    catalog.library_root = store.root.clone();
    catalog
}

/// What a catalog is once stored: only an error keeps its thumbnail state, and a
/// local library derives its content hashes.
pub(crate) fn kept(catalog: &Catalog) -> Value {
    fn fix(asset: &mut Asset) {
        if !matches!(asset.thumb.state, ThumbState::Error) {
            asset.thumb = ThumbMeta::default();
        }
        asset.content_hash = None;
        asset.variants.iter_mut().for_each(fix);
    }
    let mut copy = catalog.clone();
    copy.assets.iter_mut().for_each(fix);
    serde_json::to_value(copy).unwrap()
}

fn read_lib(store: &Store) -> Connection { Connection::open(store.library_path()).unwrap() }
fn revision_of(connection: &Connection) -> i64 {
    connection.query_row("SELECT value FROM meta WHERE key='revision'", [], |row| row.get::<_, String>(0)).map(|text| text.parse().unwrap()).unwrap_or(0)
}

#[test]
fn the_bundled_sqlite_has_fts5_with_the_trigram_tokenizer() {
    let tmp = Tmp::new("fts");
    let store = tmp.store();
    let connection = open_or_create(store.library_path(), Kind::Library).unwrap();
    let table: String = connection.query_row("SELECT name FROM sqlite_master WHERE name='asset_search'", [], |row| row.get(0)).unwrap();
    assert_eq!(table, "asset_search");
    connection.execute("INSERT INTO asset_search(rowid,name,tags) VALUES(1,'Wooden Crate','box storage')", []).unwrap();
    connection.execute("INSERT INTO asset_search(rowid,name,tags) VALUES(2,'Barrel','round')", []).unwrap();
    let found: Vec<i64> = connection.prepare("SELECT rowid FROM asset_search WHERE asset_search MATCH 'rate'").unwrap().query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(found, vec![1], "a trigram index finds the middle of a word");
    connection.execute("DELETE FROM asset_search WHERE rowid=1", []).unwrap();
    let again: i64 = connection.query_row("SELECT count(*) FROM asset_search WHERE asset_search MATCH 'rate'", [], |row| row.get(0)).unwrap();
    assert_eq!(again, 0, "rows can be deleted from the contentless table");
    let mode: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
    assert_eq!(mode, "wal");
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    let foreign_keys: i64 = connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0)).unwrap();
    assert_eq!(foreign_keys, 1);
}

#[test]
fn the_manifest_round_trips_exactly_including_unknown_keys_and_order() {
    let tmp = Tmp::new("manifest");
    let root = tmp.root();
    assert!(!manifest_exists(&root));
    assert!(read_manifest(&root).is_err());
    let doc = manifest_doc();
    write_manifest(&root, &doc).unwrap();
    assert!(manifest_exists(&root));
    let back = read_manifest(&root).unwrap();
    assert_eq!(back, doc);
    // Order too: the serialised text is identical, top-level keys and entry keys alike.
    assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&doc).unwrap());
    assert_eq!(back["assets"][0]["vendorExtension"], json!({"b": 1, "a": 2}));
    assert_eq!(back.as_object().unwrap().keys().collect::<Vec<_>>(), vec!["zebra", "schemaVersion", "storageVersion", "assets", "alpha"]);
    // A document without the optional version keys comes back without them.
    let bare = json!({"assets": [{"id": "x"}]});
    write_manifest(&root, &bare).unwrap();
    assert_eq!(serde_json::to_string(&read_manifest(&root).unwrap()).unwrap(), serde_json::to_string(&bare).unwrap());
}

#[test]
fn a_manifest_write_touches_only_what_changed_and_bumps_the_revision_once() {
    let tmp = Tmp::new("revision");
    let root = tmp.root();
    let store = tmp.store();
    let mut doc = manifest_doc();
    write_manifest(&root, &doc).unwrap();
    let connection = read_lib(&store);
    assert_eq!(revision_of(&connection), 1);
    assert_eq!(manifest_revision(&root).unwrap(), 1);
    // Writing the same document again changes nothing, not even the revision.
    write_manifest(&root, &doc).unwrap();
    assert_eq!(revision_of(&connection), 1);
    // One entry edited: one more revision, and only that row carries it.
    doc["assets"][1]["tags"] = json!(["rock"]);
    write_manifest(&root, &doc).unwrap();
    assert_eq!(revision_of(&connection), 2);
    assert_eq!(manifest_revision(&root).unwrap(), 2);
    let stamped = |connection: &Connection| -> Vec<(String, i64)> { connection.prepare("SELECT id, revision FROM items ORDER BY seq").unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect() };
    assert_eq!(stamped(&connection).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![1, 2, 1, 1, 1]);
    // An added and a removed entry: still a single step, order kept (the new one last, a gap where the old one was).
    doc["assets"].as_array_mut().unwrap().remove(0);
    doc["assets"].as_array_mut().unwrap().push(json!({"id": "uid://new", "kind": "model", "pack": "p", "category": "c", "file": "N.gltf"}));
    write_manifest(&root, &doc).unwrap();
    assert_eq!(revision_of(&connection), 3);
    assert_eq!(read_manifest(&root).unwrap(), doc);
    assert_eq!(stamped(&connection).iter().filter(|(_, r)| *r == 3).map(|(id, _)| id.as_str()).collect::<Vec<_>>(), vec!["uid://new"]);
    // A reorder renumbers and is also one step.
    doc["assets"].as_array_mut().unwrap().reverse();
    write_manifest(&root, &doc).unwrap();
    assert_eq!(revision_of(&connection), 4);
    assert_eq!(read_manifest(&root).unwrap(), doc);
    // An unknown top-level key counts as a change as well.
    doc["added"] = json!(true);
    write_manifest(&root, &doc).unwrap();
    assert_eq!(revision_of(&connection), 5);
    assert_eq!(read_manifest(&root).unwrap(), doc);
    // Invalid documents are refused and leave things as they were.
    assert!(write_manifest(&root, &json!({"assets": [{"id": "a"}, {"id": "a"}]})).is_err());
    assert!(write_manifest(&root, &json!({"assets": [{"name": "no id"}]})).is_err());
    assert_eq!(read_manifest(&root).unwrap(), doc);
    assert_eq!(revision_of(&connection), 5);
}

#[test]
fn the_catalog_round_trips_for_every_kind_with_personal_data_and_a_thumbnail_error() {
    let tmp = Tmp::new("catalog");
    let store = tmp.store();
    assert!(store.load_catalog().unwrap().is_none());
    let saved = rooted(catalog(), &store);
    store.save_catalog(&saved).unwrap();
    let back = store.load_catalog().unwrap().unwrap();
    assert_eq!(serde_json::to_value(&back).unwrap(), kept(&saved));
    assert_eq!(back.library_root, store.root);
    // The pieces the test is about, spelled out.
    let model = &back.assets[0];
    assert_eq!(model.user.provenance.as_ref().unwrap().filesystem_path.as_deref(), Some("D:/downloads/Pack"));
    assert_eq!(model.user.tags, vec!["source: Pack", "license: CC0", "mine", "also mine"]);
    assert_eq!(model.user.collections, vec!["Zoo", "Alpha"], "order is kept");
    assert!(matches!(model.thumb.state, ThumbState::Error) && model.thumb.error.as_deref() == Some("render failed"));
    assert_eq!(model.variants[0].user.collections, vec!["Low"]);
    assert!(model.variants[0].user.favorite);
    // Order of the assets and the rigs, and the audio peaks.
    assert_eq!(back.assets.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["uid://model", "uid://mat", "uid://audio", "uid://sky", "clip:walk"]);
    assert_eq!(back.rigs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["humanoid", "quad"]);

    // Personal data is in user.db, public data in library.db, and neither holds the other.
    let library = read_lib(&store);
    let blob: String = library.query_row("SELECT asset FROM assets WHERE id='uid://model'", [], |row| row.get(0)).unwrap();
    assert!(!blob.contains("D:/downloads") && !blob.contains("mine") && !blob.contains("render failed") && !blob.contains("Zoo"), "{blob}");
    assert!(blob.contains("\"license\":\"CC0\""));
    let personal = Connection::open(store.user_path()).unwrap();
    let path: String = personal.query_row("SELECT filesystem_path FROM user_assets WHERE asset_id='uid://model'", [], |row| row.get(0)).unwrap();
    assert_eq!(path, "D:/downloads/Pack");
    let tags: Vec<String> = personal.prepare("SELECT tag FROM user_tags WHERE asset_id='uid://model' ORDER BY ord").unwrap().query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(tags, vec!["mine", "also mine"], "tags derived from provenance are not stored");
    let rows: i64 = personal.query_row("SELECT count(*) FROM user_assets", [], |row| row.get(0)).unwrap();
    assert_eq!(rows, 4, "only assets with something personal have a row: the model, its variant, the material and the plugin item");
    // Indexed columns, tags and the search row.
    let (source, license, kind, type_id): (String, String, String, String) = library.query_row("SELECT source, license, kind, type FROM assets WHERE id='uid://model'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
    assert_eq!((source.as_str(), license.as_str(), kind.as_str(), type_id.as_str()), ("Pack", "CC0", "model", "model"));
    let plugin_type: String = library.query_row("SELECT type FROM assets WHERE id='uid://sky'", [], |row| row.get(0)).unwrap();
    assert_eq!(plugin_type, "skybox");
    let tag_rows: Vec<String> = library.prepare("SELECT tag FROM asset_tags WHERE asset_id='uid://model' ORDER BY tag").unwrap().query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(tag_rows, vec!["box", "wood"]);
    let hit: i64 = library.query_row("SELECT count(*) FROM asset_search WHERE asset_search MATCH 'ratch' OR asset_search MATCH 'wood'", [], |r| r.get(0)).unwrap();
    assert_eq!(hit, 1);
}

#[test]
fn tags_that_are_not_provenance_first_are_kept_verbatim() {
    let tmp = Tmp::new("verbatim");
    let store = tmp.store();
    let mut saved = rooted(catalog(), &store);
    // Provenance tags after a personal one, and a provenance tag with no provenance.
    saved.assets[0].user.tags = vec!["mine".into(), "source: Pack".into(), "license: CC0".into()];
    saved.assets[1].user.tags = vec!["source: loose".into(), "rock".into()];
    store.save_catalog(&saved).unwrap();
    let back = store.load_catalog().unwrap().unwrap();
    assert_eq!(back.assets[0].user.tags, saved.assets[0].user.tags);
    assert_eq!(back.assets[1].user.tags, saved.assets[1].user.tags);
}

#[test]
fn a_catalog_write_touches_only_changed_rows_and_records_changes() {
    let tmp = Tmp::new("diff");
    let store = tmp.store();
    let mut saved = rooted(catalog(), &store);
    let first = store.save_catalog(&saved).unwrap();
    assert!(first.changed);
    let library = read_lib(&store);
    assert_eq!(revision_of(&library), 1);
    let revisions = |library: &Connection| -> Vec<(String, i64)> { library.prepare("SELECT id, revision FROM assets ORDER BY ord").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect() };
    assert!(revisions(&library).iter().all(|(_, r)| *r == 1));

    // The same catalog again: nothing.
    let again = store.save_catalog(&saved).unwrap();
    assert!(!again.changed);
    assert_eq!(revision_of(&library), 1);

    // A personal edit changes user.db only: no library revision, no row touched.
    saved.assets[1].user.favorite = true;
    assert!(!store.save_catalog(&saved).unwrap().changed);
    assert_eq!(revision_of(&library), 1);
    assert!(store.load_catalog().unwrap().unwrap().assets[1].user.favorite);

    // A derived edit: one revision, one row.
    saved.assets[2].tags = vec!["loud".into()];
    let edit = store.save_catalog(&saved).unwrap();
    assert!(edit.changed && edit.revision == 2);
    assert_eq!(revision_of(&library), 2);
    assert_eq!(revisions(&library).iter().filter(|(_, r)| *r == 2).map(|(id, _)| id.as_str()).collect::<Vec<_>>(), vec!["uid://audio"]);
    let tag: String = library.query_row("SELECT tag FROM asset_tags WHERE asset_id='uid://audio'", [], |r| r.get(0)).unwrap();
    assert_eq!(tag, "loud");

    // Removing an asset deletes its rows and is recorded as a deletion; adding one is recorded as a change.
    let removed = saved.assets.remove(3);
    let mut added = saved.assets[0].clone();
    added.id = "uid://added".into();
    added.variants.clear();
    saved.assets.push(added);
    store.save_catalog(&saved).unwrap();
    assert_eq!(revision_of(&library), 3);
    let changes: Vec<(i64, String, i64)> = library.prepare("SELECT revision, asset_id, deleted FROM changes WHERE revision=3 ORDER BY asset_id").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(changes, vec![(3, "uid://added".to_string(), 0), (3, removed.id.clone(), 1)]);
    let left: i64 = library.query_row("SELECT count(*) FROM assets WHERE id=?1", [&removed.id], |r| r.get(0)).unwrap();
    assert_eq!(left, 0);
    let orphans: i64 = library.query_row("SELECT count(*) FROM asset_tags WHERE asset_id NOT IN (SELECT id FROM assets)", [], |r| r.get(0)).unwrap();
    assert_eq!(orphans, 0);
    let personal = Connection::open(store.user_path()).unwrap();
    let gone: i64 = personal.query_row("SELECT count(*) FROM user_assets WHERE asset_id=?1", [&removed.id], |r| r.get(0)).unwrap();
    assert_eq!(gone, 0, "personal data of an asset that left the catalog goes with it");
    // The order survived the removal.
    let back = store.load_catalog().unwrap().unwrap();
    assert_eq!(back.assets.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), saved.assets.iter().map(|a| a.id.as_str()).collect::<Vec<_>>());

    // Putting an asset back at the front reorders: still one step.
    let last = saved.assets.pop().unwrap();
    saved.assets.insert(0, last);
    store.save_catalog(&saved).unwrap();
    assert_eq!(revision_of(&library), 4);
    assert_eq!(store.load_catalog().unwrap().unwrap().assets[0].id, "uid://added");
}

#[test]
fn a_combined_commit_is_one_revision_and_the_manifest_revision_can_be_expected() {
    let tmp = Tmp::new("combined");
    let store = tmp.store();
    let saved = rooted(catalog(), &store);
    let doc = manifest_doc();
    let done = store.commit(CatalogWrite { manifest: Some(&doc), catalog: Some(&saved), expect_manifest_revision: Some(0) }).unwrap();
    assert!(done.changed && done.revision == 1);
    assert!(store.catalog_is_current().unwrap(), "derived rows were saved against this manifest");
    // The expectation protects against a change made in between.
    let mut changed = doc.clone();
    changed["assets"][0]["tags"] = json!(["x"]);
    assert!(store.commit(CatalogWrite { manifest: Some(&changed), expect_manifest_revision: Some(0), ..Default::default() }).is_err());
    assert_eq!(read_manifest(&tmp.root()).unwrap(), doc);
    store.commit(CatalogWrite { manifest: Some(&changed), expect_manifest_revision: Some(1), ..Default::default() }).unwrap();
    assert!(!store.catalog_is_current().unwrap(), "the manifest moved on; the rows are stale until saved again");
    store.save_catalog(&saved).unwrap();
    assert!(store.catalog_is_current().unwrap());
    assert_eq!(store.stamp().unwrap().revision, 2);
}

#[test]
fn a_remote_replica_keeps_content_hashes_and_has_no_manifest() {
    let tmp = Tmp::new("remote");
    let store = Store::remote_at(&tmp.0.join("files"), &tmp.cache());
    assert!(store.library_path().ends_with("appdata/library.db") || store.library_path().ends_with("appdata\\library.db"));
    let mut saved = rooted(catalog(), &store);
    saved.assets[0].content_hash = Some("top".into());
    saved.assets[0].variants[0].content_hash = Some("variant".into());
    store.save_catalog(&saved).unwrap();
    let back = store.load_catalog().unwrap().unwrap();
    assert_eq!(back.assets[0].content_hash.as_deref(), Some("top"));
    assert_eq!(back.assets[0].variants[0].content_hash.as_deref(), Some("variant"));
    let column: Option<String> = read_lib(&store).query_row("SELECT content_hash FROM assets WHERE id='uid://model'", [], |r| r.get(0)).unwrap();
    assert_eq!(column.as_deref(), Some("top"));
    assert!(store.commit(CatalogWrite { manifest: Some(&manifest_doc()), ..Default::default() }).is_err());
}

#[test]
fn content_hashes_are_a_map_by_storage_key_with_their_own_revision() {
    let tmp = Tmp::new("hashes");
    let store = tmp.store();
    let path = store.library_path().to_path_buf();
    assert!(hashes::load(&path).is_empty());
    hashes::update(&path, |map| { map.insert("models/a".into(), "h1".into()); map.insert("models/b".into(), "h2".into()); }).unwrap();
    assert_eq!(hashes::load(&path).len(), 2);
    let stamp = store.stamp().unwrap();
    assert_eq!((stamp.revision, stamp.hash_revision), (0, 1), "hashes do not move the library revision");
    // No change, no new revision.
    hashes::update(&path, |_| {}).unwrap();
    assert_eq!(store.stamp().unwrap().hash_revision, 1);
    hashes::update(&path, |map| { map.remove("models/a"); }).unwrap();
    assert_eq!(hashes::load(&path).keys().collect::<Vec<_>>(), vec!["models/b"]);
    assert_eq!(store.stamp().unwrap().hash_revision, 2);
    // A row that exists is kept in step: saving then hashing fills `assets.content_hash`.
    let saved = rooted(catalog(), &store);
    let model_key = crate::ops::content_hash::storage_key(&saved.assets[0]).unwrap();
    hashes::update(&path, |map| { map.insert(model_key.clone(), "model-hash".into()); }).unwrap();
    store.save_catalog(&saved).unwrap();
    let column = |id: &str| -> Option<String> { read_lib(&store).query_row("SELECT content_hash FROM assets WHERE id=?1", [id], |r| r.get(0)).unwrap() };
    assert_eq!(column("uid://model").as_deref(), Some("model-hash"));
    hashes::update(&path, |map| { map.insert(model_key.clone(), "newer".into()); }).unwrap();
    assert_eq!(column("uid://model").as_deref(), Some("newer"));
    // A local library's loaded catalog carries none; the side cache fills them in.
    assert!(store.load_catalog().unwrap().unwrap().assets[0].content_hash.is_none());
    // Another scheme starts over.
    let connection = Connection::open(&path).unwrap();
    connection.execute("UPDATE meta SET value='999' WHERE key='hashScheme'", []).unwrap();
    drop(connection);
    assert!(hashes::load(&path).is_empty());
    hashes::update(&path, |map| { map.insert("x/y".into(), "z".into()); }).unwrap();
    assert_eq!(hashes::load(&path).len(), 1);
    assert_eq!(column("uid://model"), None);
}

#[test]
fn a_newer_database_is_refused() {
    let tmp = Tmp::new("newer");
    let store = tmp.store();
    drop(open_or_create(store.library_path(), Kind::Library).unwrap());
    let connection = Connection::open(store.library_path()).unwrap();
    connection.execute_batch("PRAGMA user_version=99").unwrap();
    drop(connection);
    assert!(open_or_create(store.library_path(), Kind::Library).is_err());
    assert!(store.load_catalog().is_err());
}

// ---- step 4 --------------------------------------------------------------------

/// A library at migration step 3: manifest, cache and content hashes as JSON files.
fn step3_library(tmp: &Tmp) -> (Value, Catalog, Value) {
    let root = tmp.root();
    let manifest = manifest_doc();
    std::fs::create_dir_all(root.join("_library_config")).unwrap();
    std::fs::write(root.join(crate::config::SEED_REL), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    let cache = rooted(catalog(), &tmp.store());
    // As the old cache file held it: a Ready thumbnail and no fingerprints.
    let mut stored = cache.clone();
    stored.assets[1].thumb = ThumbMeta { state: ThumbState::Ready, error: None };
    std::fs::create_dir_all(tmp.cache().parent().unwrap()).unwrap();
    std::fs::write(tmp.cache(), serde_json::to_vec_pretty(&stored).unwrap()).unwrap();
    let model_key = crate::ops::content_hash::storage_key(&cache.assets[0]).unwrap();
    let hashes = json!({"scheme": 1, "hashes": {model_key: "model-hash", "skybox/other": "kept"}});
    std::fs::write(tmp.cache().with_file_name("content-hashes.json"), serde_json::to_vec(&hashes).unwrap()).unwrap();
    (manifest, cache, hashes)
}

#[test]
fn migration_step_four_builds_databases_equal_to_the_files_and_moves_the_files() {
    let tmp = Tmp::new("adopt");
    let (manifest, cache, hash_file) = step3_library(&tmp);
    assert!(adopt::adopt_local(&tmp.root(), &tmp.cache()).unwrap());
    let store = tmp.store();
    assert_eq!(read_manifest(&tmp.root()).unwrap(), manifest);
    assert_eq!(serde_json::to_value(store.load_catalog().unwrap().unwrap()).unwrap(), kept(&cache));
    assert_eq!(hashes::load(store.library_path()).get("skybox/other").map(String::as_str), Some("kept"));
    assert_eq!(hashes::load(store.library_path()).len(), 2);
    // The old files were moved into the backup folder, not deleted, and are not where they were.
    assert!(!tmp.root().join(crate::config::SEED_REL).exists());
    assert!(!tmp.cache().exists() && !tmp.cache().with_file_name("content-hashes.json").exists());
    let backups = tmp.root().join("_library_config/migration-backups");
    let dir = std::fs::read_dir(&backups).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with("0004-")).unwrap().path();
    let backed: Value = serde_json::from_slice(&std::fs::read(dir.join("manifest.before.json")).unwrap()).unwrap();
    assert_eq!(backed, manifest);
    assert!(dir.join("cache.before.json").is_file());
    let moved: Value = serde_json::from_slice(&std::fs::read(dir.join("content-hashes.before.json")).unwrap()).unwrap();
    assert_eq!(moved, hash_file);
    assert!(!store.library_path().with_file_name("library.db.new").exists());
    // Nothing left to adopt; running again changes nothing.
    let before = store.stamp().unwrap();
    assert!(!adopt::adopt_local(&tmp.root(), &tmp.cache()).unwrap());
    assert_eq!(store.stamp().unwrap(), before);
}

#[test]
fn migration_step_four_can_be_repeated_after_a_crash_at_any_point() {
    // A leftover library.db.new (crash while building) is discarded.
    let tmp = Tmp::new("crash-build");
    let (manifest, cache, _) = step3_library(&tmp);
    std::fs::write(tmp.root().join("_library_config/library.db.new"), b"half a database").unwrap();
    assert!(adopt::adopt_local(&tmp.root(), &tmp.cache()).unwrap());
    assert_eq!(read_manifest(&tmp.root()).unwrap(), manifest);
    assert_eq!(serde_json::to_value(tmp.store().load_catalog().unwrap().unwrap()).unwrap(), kept(&cache));
    assert!(!tmp.root().join("_library_config/library.db.new").exists());

    // Crash after the rename, before any file moved: the finished database is kept and the files are moved.
    let tmp = Tmp::new("crash-rename");
    let (manifest, cache, _) = step3_library(&tmp);
    let store = tmp.store();
    {
        let side = tmp.root().join("_library_config/library.db.new");
        let side_store = Store { library: side.clone(), ..store.clone() };
        side_store.commit(CatalogWrite { manifest: Some(&manifest), catalog: Some(&cache), expect_manifest_revision: None }).unwrap();
        let connection = open_existing(&side, Kind::Library).unwrap().unwrap();
        connection.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get::<_, String>(0)).unwrap();
        drop(connection);
        std::fs::rename(&side, store.library_path()).unwrap();
    }
    assert!(tmp.root().join(crate::config::SEED_REL).exists(), "the crash left the old files in place");
    assert!(adopt::adopt_local(&tmp.root(), &tmp.cache()).unwrap());
    assert!(!tmp.root().join(crate::config::SEED_REL).exists() && !tmp.cache().exists());
    assert_eq!(read_manifest(&tmp.root()).unwrap(), manifest);
    assert_eq!(serde_json::to_value(store.load_catalog().unwrap().unwrap()).unwrap(), kept(&cache));

    // Crash after the cache moved but before the manifest did.
    let tmp = Tmp::new("crash-move");
    let (manifest, _, _) = step3_library(&tmp);
    assert!(adopt::adopt_local(&tmp.root(), &tmp.cache()).unwrap());
    let backup = std::fs::read_dir(tmp.root().join("_library_config/migration-backups")).unwrap().flatten().next().unwrap().path();
    std::fs::rename(backup.join("manifest.before.json"), tmp.root().join(crate::config::SEED_REL)).unwrap();
    assert!(adopt::adopt_local(&tmp.root(), &tmp.cache()).unwrap(), "the manifest that is still there is moved on");
    assert!(!tmp.root().join(crate::config::SEED_REL).exists());
    assert_eq!(read_manifest(&tmp.root()).unwrap(), manifest);
}

#[test]
fn a_cache_of_another_folder_or_no_cache_leaves_the_catalog_to_be_rebuilt() {
    let tmp = Tmp::new("adopt-foreign");
    let (manifest, _, _) = step3_library(&tmp);
    let mut foreign: Value = serde_json::from_slice(&std::fs::read(tmp.cache()).unwrap()).unwrap();
    foreign["libraryRoot"] = json!("Z:/somewhere/else");
    std::fs::write(tmp.cache(), serde_json::to_vec(&foreign).unwrap()).unwrap();
    assert!(adopt::adopt_local(&tmp.root(), &tmp.cache()).unwrap());
    assert_eq!(read_manifest(&tmp.root()).unwrap(), manifest);
    assert!(tmp.store().load_catalog().unwrap().is_none(), "no catalog is claimed; a scan builds it");
    assert!(!tmp.store().catalog_exists());

    let tmp = Tmp::new("adopt-nocache");
    let root = tmp.root();
    std::fs::create_dir_all(root.join("_library_config")).unwrap();
    std::fs::write(root.join(crate::config::SEED_REL), serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(adopt::adopt_local(&root, &tmp.cache()).unwrap());
    assert!(tmp.store().load_catalog().unwrap().is_none());

    // A cache that cannot be read stops the migration with the files untouched.
    let tmp = Tmp::new("adopt-broken");
    let root = tmp.root();
    std::fs::create_dir_all(root.join("_library_config")).unwrap();
    std::fs::write(root.join(crate::config::SEED_REL), serde_json::to_vec(&manifest).unwrap()).unwrap();
    std::fs::create_dir_all(tmp.cache().parent().unwrap()).unwrap();
    std::fs::write(tmp.cache(), b"{ not json").unwrap();
    assert!(adopt::adopt_local(&root, &tmp.cache()).is_err());
    assert!(root.join(crate::config::SEED_REL).exists() && tmp.cache().exists() && !tmp.store().library_path().exists());
}

#[test]
fn migration_step_four_fills_the_reference_tables_the_incremental_writes_rely_on() {
    let tmp = Tmp::new("adopt-refs");
    let (_, cache, _) = step3_library(&tmp);
    assert!(adopt::adopt_local(&tmp.root(), &tmp.cache()).unwrap());
    let library = read_lib(&tmp.store());
    let n = |sql: &str| -> i64 { library.query_row(sql, [], |row| row.get(0)).unwrap() };
    // Every asset's files are listed (what a delete asks of the table), with kept sources marked.
    for asset in &cache.assets {
        let mut wanted = Vec::new();
        asset.collect_files(&mut wanted);
        wanted.sort();
        wanted.dedup();
        let mut listed: Vec<String> = library.prepare("SELECT path FROM asset_files WHERE asset_id=?1 ORDER BY path").unwrap().query_map([&asset.id], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
        listed.sort();
        assert_eq!(listed, wanted, "{}", asset.id);
    }
    assert_eq!(n("SELECT COUNT(*) FROM items WHERE pkey IS NOT NULL"), n("SELECT COUNT(*) FROM items"));
    assert_eq!(n("SELECT COUNT(*) FROM assets WHERE rel_path = '' OR thumb_name = ''"), 0);
    let variants: usize = cache.assets.iter().map(|asset| asset.variants.len()).sum();
    let in_manifest: usize = manifest_doc()["assets"].as_array().unwrap().iter().map(|entry| entry["variants"].as_array().map_or(0, Vec::len)).sum();
    assert_eq!(n("SELECT COUNT(*) FROM item_variants") as usize, in_manifest);
    let _ = variants;
    let rig_uses: usize = cache.assets.iter().map(|asset| asset.rigs.len() + asset.variants.iter().map(|v| v.rigs.len()).sum::<usize>()).sum();
    assert_eq!(n("SELECT COUNT(*) FROM rig_uses") as usize, rig_uses);
    assert_eq!(n("SELECT COUNT(*) FROM rigs WHERE shape = '' OR kind = ''") , 0);
}

#[test]
fn a_delta_leaves_the_same_rows_a_whole_save_would() {
    // The same library written whole (manifest, then catalog) and item by item.
    let whole = Tmp::new("whole");
    let by_delta = Tmp::new("by-delta");
    let manifest = manifest_doc();
    let mut catalog = rooted(catalog(), &whole.store());
    // Only assets that come from an item (a delta writes the assets of items).
    let item_ids: Vec<String> = manifest["assets"].as_array().unwrap().iter().map(|entry| entry["id"].as_str().unwrap().to_string()).collect();
    catalog.assets.retain(|asset| item_ids.contains(&asset.id));
    whole.store().commit(CatalogWrite { manifest: Some(&manifest), catalog: Some(&catalog), expect_manifest_revision: None }).unwrap();
    let store = by_delta.store();
    // An empty manifest first, then every item as a put.
    write_manifest(&by_delta.root(), &json!({"schemaVersion":1,"storageVersion":1,"assets":[]})).unwrap();
    let entries = manifest["assets"].as_array().unwrap().clone();
    let puts = entries
        .iter()
        .map(|entry| {
            let id = entry["id"].as_str().unwrap().to_string();
            let assets = catalog.assets.iter().filter(|asset| asset.id == id).cloned().collect();
            ItemPut { id, entry: Some(entry.clone()), assets, rigs: catalog.rigs.clone() }
        })
        .collect();
    store.apply(&Delta { puts, ..Default::default() }).unwrap();
    // Same rows in every table the incremental writers keep (revisions and rowids aside).
    let dump = |store: &Store, sql: &str| -> Vec<String> {
        let connection = read_lib(store);
        let mut statement = connection.prepare(sql).unwrap();
        let columns = statement.column_count();
        let rows = statement
            .query_map([], |row| {
                Ok((0..columns)
                    .map(|index| match row.get_ref(index).unwrap() {
                        rusqlite::types::ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).to_string(),
                        rusqlite::types::ValueRef::Integer(number) => number.to_string(),
                        other => format!("{other:?}"),
                    })
                    .collect::<Vec<_>>()
                    .join("|"))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    for sql in [
        "SELECT id, kind, type, pack, category, name, storage_path, entry, pkey FROM items ORDER BY seq",
        "SELECT variant_id, item_id, thumb_name FROM item_variants ORDER BY variant_id, item_id",
        "SELECT id, kind, type, name, pack, category, format, source, publisher, license, asset, rel_path, thumb_name, clip_source FROM assets ORDER BY id",
        "SELECT asset_id, tag FROM asset_tags ORDER BY asset_id, tag",
        "SELECT path, asset_id, origin FROM asset_files ORDER BY path, asset_id",
        "SELECT rig_id, asset_id FROM rig_uses ORDER BY rig_id, asset_id",
        "SELECT id, rig, kind, shape FROM rigs ORDER BY rowid",
    ] {
        assert_eq!(dump(&whole.store(), sql), dump(&store, sql), "{sql}");
    }
    let normal = |store: &Store| {
        let mut doc = serde_json::to_value(store.load_catalog().unwrap().unwrap()).unwrap();
        doc["libraryRoot"] = json!("");
        doc["assets"].as_array_mut().unwrap().sort_by_key(|asset| asset["id"].as_str().unwrap().to_string());
        doc
    };
    assert_eq!(normal(&whole.store()), normal(&store));
}
