//! Changing a remote library: import by upload, delete, and what a read-only
//! token or a missing server does to both.

mod common;

use std::sync::atomic::Ordering;

use common::*;
use serde_json::{json, Value};
use toybox_core::ops::catalog::{load_catalog_inner, save_user_catalog};
use toybox_core::ops::transfer::{OnConflict, PackageAsset, PackageFile, TransferPackage};
use toybox_remote::{RemoteError, RemoteLibrary};
use toybox_server::config::Role;

fn stage(library: &RemoteLibrary, files: &[(&str, Vec<u8>)]) {
    for (rel, bytes) in files {
        let path = library.files_root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}

fn staging_left(library: &RemoteLibrary) -> bool {
    std::fs::read_dir(library.files_root().join("library")).map(|entries| entries.flatten().any(|e| e.file_name().to_string_lossy().starts_with("_import_"))).unwrap_or(false)
}

#[test]
fn an_import_uploads_what_was_staged_and_the_library_has_it_afterwards() {
    let server = TestServer::start();
    let mut world = World::new("import");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let (files, entries) = standard_content("up1");
    stage(&library, &files);
    assert!(staging_left(&library));

    let outcome = library.import(entries).unwrap();
    let mut imported = outcome.imported.clone();
    imported.sort();
    assert_eq!(imported, vec!["uid://barrel", "uid://click", "uid://crate", "uid://night", "uid://stone"]);
    assert!(outcome.errors.is_empty());
    assert_eq!(outcome.catalog.assets.len(), 5, "the returned catalog is the refreshed one");
    assert_eq!(server.ids(), vec!["uid://barrel", "uid://click", "uid://crate", "uid://night", "uid://stone"]);
    // The staging is gone from this computer, the upload from the server.
    assert!(!staging_left(&library));
    assert_eq!(server.state.uploads.count(), 0);
    // What arrived is what was staged, stored by the server like any import.
    let crate_item = outcome.catalog.assets.iter().find(|a| a.id == "uid://crate").unwrap();
    let stored = server.library_root().join(&crate_item.fileset().unwrap().textures[0]);
    assert_eq!(std::fs::read(stored).unwrap(), png());
    // Progress was reported, ending with everything sent.
    let events: Vec<_> = world.progress().into_iter().filter(|p| p.operation == "upload").collect();
    assert!(!events.is_empty());
    let last = events.last().unwrap();
    assert_eq!((last.done_files, last.total_files), (files.len() as u64, files.len() as u64));
    assert!(last.done_bytes > 0 && last.done_bytes == last.total_bytes);
}

#[test]
fn entries_the_server_refuses_are_reported_one_by_one_and_the_rest_are_imported() {
    let server = TestServer::start();
    let mut world = World::new("partial");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    stage(&library, &[("library/_import_p1/sounds/ok.wav", wav())]);
    let entries = vec![
        json!({"id":"uid://ok","kind":"audio","pack":"_import_p1","category":"sounds","file":"ok.wav"}),
        json!({"id":"uid://missing","kind":"audio","pack":"_import_p1","category":"sounds","file":"never-staged.wav"}),
        json!({"id":"uid://sneaky","kind":"audio","pack":"_import_p1","category":"../../outside","file":"ok.wav"}),
    ];
    let outcome = library.import(entries).unwrap();
    assert_eq!(outcome.imported, vec!["uid://ok"]);
    assert_eq!(outcome.errors.len(), 2);
    let missing = outcome.errors.iter().find(|e| e.id.as_deref() == Some("uid://missing")).unwrap();
    assert!(missing.message.contains("never-staged.wav") || missing.message.contains("missing"), "{}", missing.message);
    let sneaky = outcome.errors.iter().find(|e| e.id.as_deref() == Some("uid://sneaky")).unwrap();
    assert!(sneaky.message.contains("plain relative path"), "{}", sneaky.message);
    assert_eq!(server.ids(), vec!["uid://ok"]);
    assert!(outcome.catalog.assets.iter().any(|a| a.id == "uid://ok"));
    assert!(!staging_left(&library));
    assert_eq!(server.state.uploads.count(), 0, "a partly failed upload is cancelled, not left on the server");

    // Entries that do not say where their files are cannot be uploaded at all.
    let error = library.import(vec![json!({"id":"uid://x","kind":"audio","pack":"library","file":"a.wav"})]).unwrap_err();
    assert!(error.to_string().contains("staging folder"), "{error}");
}

#[test]
fn replacing_an_item_through_an_import_updates_it_in_place() {
    let server = TestServer::start();
    let mut world = World::new("replace");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    stage(&library, &[("library/_import_r1/sounds/a.wav", wav())]);
    library.import(vec![json!({"id":"uid://a","kind":"audio","pack":"_import_r1","category":"sounds","file":"a.wav"})]).unwrap();
    let mut louder = wav();
    louder[60] = 9;
    stage(&library, &[("library/_import_r2/sounds/a.wav", louder.clone())]);
    let outcome = library.import(vec![json!({"id":"uid://a","kind":"audio","pack":"_import_r2","category":"sounds","file":"a.wav"})]).unwrap();
    assert_eq!(outcome.imported, vec!["uid://a"]);
    assert_eq!(outcome.catalog.assets.len(), 1);
    let file = match &outcome.catalog.assets[0].content {
        toybox_core::domain::catalog_model::AssetContent::Audio { audio } => audio.file.clone(),
        _ => panic!("audio"),
    };
    assert_eq!(std::fs::read(server.library_root().join(file)).unwrap(), louder);
}

#[test]
fn a_read_only_token_cannot_write_and_each_refusal_says_why() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("readonly");
    let library = world.add_loaded(&server, &server.token("friend", Role::Read), "Friend");
    assert!(!library.can_write());

    let (files, entries) = standard_content("no");
    stage(&library, &files);
    let error = library.import(entries).unwrap_err();
    assert!(matches!(error, RemoteError::Forbidden(_)) && error.to_string().contains("read-only"), "{error}");
    let error = library.delete_assets(&["uid://crate".to_string()]).unwrap_err();
    assert!(error.to_string().contains("read-only"), "{error}");
    assert_eq!(server.ids().len(), 5, "nothing was deleted");

    // And the server itself says no when a client is built to ignore the rules.
    let client = library.client();
    let upload = client.create_upload().unwrap_err();
    assert!(matches!(upload, RemoteError::Forbidden(_)) && upload.to_string().contains("write role"), "{upload}");
    let delete = client.delete_assets(&["uid://crate".to_string()]).unwrap_err();
    assert!(matches!(delete, RemoteError::Forbidden(_)), "{delete}");
    assert!(matches!(client.put_thumb("uid://crate", &png()).unwrap_err(), RemoteError::Forbidden(_)));
    assert!(matches!(client.tokens().unwrap_err(), RemoteError::Forbidden(_)));
    // Library tags and sources are edits too: refused here, and by the server itself.
    let tag = toybox_core::ops::metadata::MetadataEdit { id: "uid://crate".into(), tags: Some(vec!["nope".into()]), provenance: None };
    let error = library.update_metadata(std::slice::from_ref(&tag)).unwrap_err();
    assert!(matches!(error, RemoteError::Forbidden(_)) && error.to_string().contains("read-only"), "{error}");
    assert!(matches!(client.update_metadata(std::slice::from_ref(&tag)).unwrap_err(), RemoteError::Forbidden(_)));
    assert!(server.catalog().assets.iter().find(|a| a.id == "uid://crate").unwrap().tags != vec!["nope"]);
    // Reading is fine.
    assert!(client.info().is_ok() && client.catalog(None).is_ok());
    assert_eq!(library.stats.thumb_uploads.load(Ordering::Relaxed), 0);
}

#[test]
fn deleting_removes_the_items_their_unshared_files_and_pictures_here_and_there() {
    let server = TestServer::start();
    server.seed();
    toybox_core::ops::thumbs::save_thumb(&server.handle(), "uid://crate", &png()).unwrap();
    let mut world = World::new("delete");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let handle = library.handle();
    let catalog = load_catalog_inner(&handle).unwrap().unwrap();
    let crate_item = catalog.assets.iter().find(|a| a.id == "uid://crate").unwrap().clone();
    let barrel = catalog.assets.iter().find(|a| a.id == "uid://barrel").unwrap().clone();
    handle.ensure_assets(&[crate_item.clone(), barrel.clone()]).unwrap();
    handle.remote.as_ref().unwrap().files.ensure_thumb("uid://crate").unwrap();
    // Barrel's picture choice points at the crate, which is about to go.
    let mut mine = load_catalog_inner(&handle).unwrap().unwrap();
    mine.assets.iter_mut().find(|a| a.id == "uid://barrel").unwrap().user.preview_asset_id = Some("uid://crate".into());
    save_user_catalog(&handle, mine).unwrap();

    let crate_files = crate_item.fileset().unwrap().clone();
    // Images are stored by content: the barrel uses one of the crate's two.
    let shared_image = barrel.fileset().unwrap().textures[0].clone();
    assert!(crate_files.textures.contains(&shared_image));
    let crate_only_image = crate_files.textures.iter().find(|t| **t != shared_image).unwrap().clone();
    let after = library.delete_assets(&["uid://crate".to_string()]).unwrap();

    assert_eq!(server.ids(), vec!["uid://barrel", "uid://click", "uid://night", "uid://stone"]);
    assert!(after.assets.iter().all(|a| a.id != "uid://crate"));
    assert!(!server.handle().thumb_path("uid://crate").exists(), "the server dropped the shared picture");
    assert!(!handle.thumb_path("uid://crate").exists(), "and so did this computer");
    let local = |rel: &str| library.files_root().join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    assert!(!local(&crate_files.gltf).exists() && !local(&crate_files.bin).exists(), "the crate's own files are gone from the cache");
    assert!(local(&shared_image).exists(), "the image the barrel shares stays");
    assert!(!local(&crate_only_image).exists());
    let barrel_now = after.assets.iter().find(|a| a.id == "uid://barrel").unwrap();
    assert_eq!(barrel_now.user.preview_asset_id, None, "a choice that named the deleted item is cleared");
    assert_eq!(load_catalog_inner(&handle).unwrap().unwrap().assets.len(), 4);

    // An unknown id deletes nothing.
    let error = library.delete_assets(&["uid://barrel".to_string(), "uid://nope".to_string()]).unwrap_err();
    assert!(matches!(error, RemoteError::NotFound(_)), "{error:?}");
    assert_eq!(server.ids().len(), 4);
}

#[test]
fn nothing_can_be_written_while_the_server_is_away() {
    let mut server = TestServer::start();
    let mut world = World::new("away");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    server.shutdown();
    library.connect();
    let error = library.import(vec![]).unwrap_err();
    assert!(error.to_string().contains("offline"), "{error}");
    assert!(library.delete_assets(&["uid://x".to_string()]).unwrap_err().to_string().contains("offline"));
}

fn package_with(entry: Value, files: Vec<PackageFile>) -> TransferPackage {
    TransferPackage {
        version: 1,
        source_library: String::new(),
        assets: vec![PackageAsset { id: entry["id"].as_str().unwrap().to_string(), name: "x".into(), kind: "audio".into(), entry, files, user: Default::default(), thumbs: vec![] }],
        failed: vec![],
    }
}

#[test]
fn a_transfer_package_cannot_name_paths_outside_its_staging_or_forge_files() {
    let server = TestServer::start();
    let mut world = World::new("hostile");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let client = library.client();
    let sha = |bytes: &[u8]| {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes))
    };
    let audio = wav();
    let file = PackageFile { path: "library/audio/0123456789abcdef/a.wav".into(), size: audio.len() as u64, sha256: sha(&audio) };

    // Names that would climb out of the staging folder are refused before core sees them.
    for entry in [
        json!({"id":"uid://evil1","kind":"audio","pack":"p","category":"c","file":"a.wav","name":"../../escape"}),
        json!({"id":"uid://evil2","kind":"audio","pack":"../p","category":"c","file":"a.wav"}),
        json!({"id":"uid://evil3","kind":"plugin","type":"../x","pack":"p","category":"c","name":"n","files":["a.wav"]}),
    ] {
        let upload = client.create_upload().unwrap();
        client.put_package_file(&upload.id, &file.path, &mut std::io::Cursor::new(audio.clone()), audio.len() as u64).unwrap();
        let reply = client.ingest(&upload.id, &package_with(entry, vec![file.clone()]), OnConflict::Skip).unwrap();
        assert_eq!(reply.report.items.len(), 1);
        assert_eq!(format!("{:?}", reply.report.items[0].outcome), "Failed", "{:?}", reply.report.items[0]);
        assert!(reply.report.items[0].reason.as_deref().unwrap().contains("plain"), "{:?}", reply.report.items[0].reason);
    }
    // A package path outside library/ and thumbs/ cannot even be uploaded.
    let upload = client.create_upload().unwrap();
    for bad in ["_library_config/catalog.json", "library/../x.wav", "thumbs/sub/x.png", "other/a.wav", "library/x"] {
        assert!(client.put_package_file(&upload.id, bad, &mut std::io::Cursor::new(b"x".to_vec()), 1).is_err(), "{bad}");
    }
    // A file that does not match its announced digest fails its item and writes nothing.
    let entry = json!({"id":"uid://forged","kind":"audio","pack":"p","category":"c","file":"a.wav"});
    let forged = PackageFile { sha256: sha(b"something else"), ..file.clone() };
    client.put_package_file(&upload.id, &file.path, &mut std::io::Cursor::new(audio.clone()), audio.len() as u64).unwrap();
    let reply = client.ingest(&upload.id, &package_with(entry, vec![forged]), OnConflict::Skip).unwrap();
    assert_eq!(format!("{:?}", reply.report.items[0].outcome), "Failed");
    assert!(server.ids().is_empty());
    assert!(!server.library_root().join("escape").exists());
}

#[test]
fn the_staged_files_of_a_failed_upload_do_not_survive_on_the_server() {
    let server = TestServer::start();
    let mut world = World::new("cancel");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let client = library.client();
    let upload = client.create_upload().unwrap();
    assert_eq!(server.state.uploads.count(), 1);
    client.cancel_upload(&upload.id).unwrap();
    assert_eq!(server.state.uploads.count(), 0);
    assert!(client.cancel_upload(&upload.id).is_err());
}

#[test]
fn library_tags_and_sources_are_edited_on_the_server_and_this_cache_follows_without_a_download() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("metadata");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    // Personal data kept here for the remote library must survive an edit.
    let mut mine = load_catalog_inner(&library.handle()).unwrap().unwrap();
    {
        let item = mine.assets.iter_mut().find(|a| a.id == "uid://crate").unwrap();
        item.user.favorite = true;
        item.user.tags.push("mine".into());
    }
    save_user_catalog(&library.handle(), mine).unwrap();
    let downloads = library.stats.catalog_downloads.load(Ordering::Relaxed);

    let edits: Vec<toybox_core::ops::metadata::MetadataEdit> = serde_json::from_value(json!([
        {"id":"uid://crate","tags":["Footstep","wood"],"provenance":{"source":"Better Pack","license":null,"url":"https://example.test/crate"}},
        {"id":"uid://crate_lod","tags":["lod"]},
        {"id":"uid://click","tags":["Coin"]},
    ])).unwrap();
    let touched = library.update_metadata(&edits).unwrap();
    let mut ids: Vec<&str> = touched.iter().map(|a| a.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["uid://click", "uid://crate"], "a variant comes inside its parent");
    let crate_item = touched.iter().find(|a| a.id == "uid://crate").unwrap();
    assert_eq!(crate_item.tags, vec!["footstep", "wood"]);
    assert_eq!(crate_item.variants[0].tags, vec!["lod"]);
    assert!(crate_item.user.favorite && crate_item.user.tags.contains(&"mine".to_string()), "personal data stays");
    assert!(crate_item.user.tags.contains(&"source: Better Pack".to_string()) && !crate_item.user.tags.contains(&"source: Crate Pack".to_string()));
    assert!(!crate_item.user.tags.iter().any(|t| t.starts_with("license:")));

    // On the server: the manifest and the catalog it serves.
    let on_server = server.catalog();
    let served = on_server.assets.iter().find(|a| a.id == "uid://crate").unwrap();
    assert_eq!(served.tags, vec!["footstep", "wood"]);
    assert_eq!(served.user.provenance.as_ref().unwrap().url.as_deref(), Some("https://example.test/crate"));
    assert_eq!(on_server.assets.iter().find(|a| a.id == "uid://click").unwrap().tags, vec!["coin"]);

    // Here: the cache already says so, and nothing was downloaded to learn it.
    let cache = load_catalog_inner(&library.handle()).unwrap().unwrap();
    assert_eq!(cache.assets.iter().find(|a| a.id == "uid://click").unwrap().tags, vec!["coin"]);
    assert_eq!(library.stats.catalog_downloads.load(Ordering::Relaxed), downloads);
    // The reply was applied as it came and the cursor moved with it: the next refresh finds nothing
    // newer to download, and the catalog it returns keeps the personal part.
    let refreshed = library.refresh_catalog(false).unwrap();
    assert_eq!(library.stats.catalog_downloads.load(Ordering::Relaxed), downloads);
    let again = refreshed.assets.iter().find(|a| a.id == "uid://crate").unwrap();
    assert_eq!(again.tags, vec!["footstep", "wood"]);
    assert!(again.user.favorite && again.user.tags.contains(&"mine".to_string()));
    assert!(!again.user.tags.contains(&"source: Crate Pack".to_string()));

    // Clearing a source entirely clears it here too, on the next refresh.
    let clear: Vec<toybox_core::ops::metadata::MetadataEdit> = serde_json::from_value(json!([
        {"id":"uid://crate","provenance":{"source":null,"url":null}}
    ])).unwrap();
    library.update_metadata(&clear).unwrap();
    let refreshed = library.refresh_catalog(false).unwrap();
    let cleared = refreshed.assets.iter().find(|a| a.id == "uid://crate").unwrap();
    assert!(cleared.user.provenance.is_none(), "{:?}", cleared.user.provenance);
    assert!(!cleared.user.tags.iter().any(|t| t.starts_with("source:")) && cleared.user.tags.contains(&"mine".to_string()));
}

#[test]
fn editing_tags_of_an_offline_remote_library_is_refused_and_changes_nothing() {
    let mut server = TestServer::start();
    server.seed();
    let mut world = World::new("metadata-offline");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    server.shutdown();
    library.connect();
    assert!(library.is_offline());
    let edit = toybox_core::ops::metadata::MetadataEdit { id: "uid://crate".into(), tags: Some(vec!["x".into()]), provenance: None };
    let error = library.update_metadata(&[edit]).unwrap_err();
    assert!(error.to_string().contains("offline"), "{error}");
    let cache = load_catalog_inner(&library.handle()).unwrap().unwrap();
    assert_eq!(cache.assets.iter().find(|a| a.id == "uid://crate").unwrap().tags, vec!["wooden crate", "prop"]);
}

fn sha_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// `count` small audio items, one file each, with their content to read from.
fn many_audio(count: usize) -> (TransferPackage, std::collections::HashMap<String, Vec<u8>>) {
    let mut contents = std::collections::HashMap::new();
    let mut assets = Vec::new();
    for i in 0..count {
        let mut bytes = wav();
        bytes.extend(format!("item {i}").as_bytes());
        let sha = sha_hex(&bytes);
        let path = format!("library/audio/{sha}/s{i}.wav");
        let entry = json!({"id": format!("uid://many{i}"), "kind": "audio", "pack": "imported", "category": "sounds", "file": format!("s{i}.wav"), "storagePath": path});
        let file = PackageFile { path: path.clone(), size: bytes.len() as u64, sha256: sha };
        assets.push(PackageAsset { id: format!("uid://many{i}"), name: format!("s{i}.wav"), kind: "audio".into(), entry, files: vec![file], user: Default::default(), thumbs: vec![] });
        contents.insert(path, bytes);
    }
    (TransferPackage { version: 1, source_library: String::new(), assets, failed: vec![] }, contents)
}

#[test]
fn a_package_of_many_small_files_is_uploaded_in_parallel_and_arrives_intact_with_steady_progress() {
    let server = TestServer::start();
    let mut world = World::new("parallel");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let (package, contents) = many_audio(80);
    let seen = std::cell::RefCell::new(Vec::new());
    let read = |path: &str| -> toybox_core::error::AppResult<Box<dyn std::io::Read>> { Ok(Box::new(std::io::Cursor::new(contents[path].clone()))) };
    let report = library.upload_package(&package, OnConflict::Skip, &read, &|p| seen.borrow_mut().push((p.done, p.total))).unwrap();
    assert_eq!(report.items.len(), 80);
    assert!(report.items.iter().all(|item| format!("{:?}", item.outcome) == "Copied"), "{:?}", report.items.iter().find(|i| format!("{:?}", i.outcome) != "Copied"));

    // Every file arrived with its bytes.
    let catalog = server.catalog();
    assert_eq!(catalog.assets.len(), 80);
    // (The server may key the folder differently; find each file by name.)
    let mut stored: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    for folder in std::fs::read_dir(server.library_root().join("library/audio")).unwrap().flatten() {
        for file in std::fs::read_dir(folder.path()).unwrap().flatten() {
            stored.insert(file.file_name().to_string_lossy().into_owned(), std::fs::read(file.path()).unwrap());
        }
    }
    assert_eq!(stored.len(), 80);
    for (path, bytes) in &contents {
        let name = path.rsplit('/').next().unwrap();
        assert_eq!(stored.get(name), Some(bytes), "{path}");
    }
    assert_eq!(server.state.uploads.count(), 0);

    // Item progress never goes back and ends complete; so does the file/byte progress.
    let seen = seen.into_inner();
    assert!(seen.iter().all(|(done, total)| *total == 80 && done <= total));
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
    assert_eq!(*seen.last().unwrap(), (80, 80));
    let events: Vec<_> = world.progress().into_iter().filter(|p| p.operation == "upload").collect();
    assert!(events.windows(2).all(|w| w[0].done_files <= w[1].done_files && w[0].done_bytes <= w[1].done_bytes));
    let last = events.last().unwrap();
    assert_eq!((last.done_files, last.total_files), (80, 80));
    assert!(last.done_bytes > 0 && last.done_bytes == last.total_bytes);
}

#[test]
fn a_file_the_server_refuses_midway_stops_the_upload_and_removes_it() {
    let server = TestServer::start();
    let mut world = World::new("parallel-fail");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let (mut package, mut contents) = many_audio(60);
    // The server never accepts a path ending in .tbx-part.
    let bad = "library/audio/0123456789abcdef/refused.tbx-part".to_string();
    package.assets[30].files.push(PackageFile { path: bad.clone(), size: 3, sha256: sha_hex(b"abc") });
    contents.insert(bad, b"abc".to_vec());
    let read = |path: &str| -> toybox_core::error::AppResult<Box<dyn std::io::Read>> { Ok(Box::new(std::io::Cursor::new(contents[path].clone()))) };
    let error = library.upload_package(&package, OnConflict::Skip, &read, &|_| {}).unwrap_err();
    assert!(matches!(error, RemoteError::Rejected { .. }), "{error:?}");
    assert!(server.ids().is_empty(), "nothing was ingested");
    // The upload is cancelled on the server (give a hung-up write a moment to be noticed).
    for _ in 0..50 {
        if server.state.uploads.count() == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(server.state.uploads.count(), 0);
    // The library can be used again straight away.
    let (good, contents) = many_audio(3);
    let read = |path: &str| -> toybox_core::error::AppResult<Box<dyn std::io::Read>> { Ok(Box::new(std::io::Cursor::new(contents[path].clone()))) };
    assert_eq!(library.upload_package(&good, OnConflict::Skip, &read, &|_| {}).unwrap().items.len(), 3);
}

#[test]
fn a_server_that_goes_away_during_a_parallel_upload_fails_it_and_marks_the_library_offline() {
    let mut server = TestServer::start();
    let mut world = World::new("parallel-away");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let (package, contents) = many_audio(40);
    let count = std::cell::Cell::new(0);
    // Reading the 10th file is the moment the server stops.
    let server_cell = std::cell::RefCell::new(Some(&mut server));
    let read = |path: &str| -> toybox_core::error::AppResult<Box<dyn std::io::Read>> {
        count.set(count.get() + 1);
        if count.get() == 10 {
            if let Some(server) = server_cell.borrow_mut().take() {
                server.shutdown();
            }
        }
        Ok(Box::new(std::io::Cursor::new(contents[path].clone())))
    };
    let error = library.upload_package(&package, OnConflict::Skip, &read, &|_| {}).unwrap_err();
    assert!(error.is_connectivity(), "{error:?}");
    assert!(library.is_offline(), "the library went offline");
}

#[test]
fn an_import_of_many_staged_files_goes_up_in_parallel_with_steady_progress() {
    let server = TestServer::start();
    let mut world = World::new("import-many");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let mut files = Vec::new();
    let mut entries = Vec::new();
    for i in 0..70 {
        let mut bytes = wav();
        bytes.extend(format!("clip {i}").as_bytes());
        files.push((format!("library/_import_many/sounds/c{i}.wav"), bytes));
        entries.push(json!({"id": format!("uid://c{i}"), "kind": "audio", "pack": "_import_many", "category": "sounds", "file": format!("c{i}.wav")}));
    }
    let staged: Vec<(&str, Vec<u8>)> = files.iter().map(|(rel, bytes)| (rel.as_str(), bytes.clone())).collect();
    stage(&library, &staged);
    let outcome = library.import(entries).unwrap();
    assert_eq!(outcome.imported.len(), 70);
    assert!(outcome.errors.is_empty());
    assert_eq!(server.ids().len(), 70);
    assert_eq!(server.state.uploads.count(), 0);
    let events: Vec<_> = world.progress().into_iter().filter(|p| p.operation == "upload").collect();
    assert!(events.windows(2).all(|w| w[0].done_files <= w[1].done_files && w[0].done_bytes <= w[1].done_bytes));
    let last = events.last().unwrap();
    assert_eq!((last.done_files, last.total_files), (70, 70));
    assert_eq!(last.done_bytes, last.total_bytes);
}

/// The replica's asset rows with the revision that last wrote them.
fn replica_revisions(library: &RemoteLibrary) -> std::collections::BTreeMap<String, i64> {
    let connection = rusqlite::Connection::open(library.handle().library_db_path()).unwrap();
    let mut statement = connection.prepare("SELECT id, revision FROM assets").unwrap();
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).unwrap();
    rows.map(Result::unwrap).collect()
}

#[test]
fn an_edit_and_a_delete_write_only_the_rows_of_the_items_they_name_into_the_replica() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("rows");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let before = replica_revisions(&library);
    assert_eq!(before.len(), 5);

    let edits: Vec<toybox_core::ops::metadata::MetadataEdit> = serde_json::from_value(json!([{"id":"uid://click","tags":["Coin"]}])).unwrap();
    library.update_metadata(&edits).unwrap();
    let after = replica_revisions(&library);
    let changed: Vec<&String> = after.iter().filter(|(id, revision)| before.get(*id) != Some(revision)).map(|(id, _)| id).collect();
    assert_eq!(changed, ["uid://click"], "the other rows were not written");

    library.delete_assets(&["uid://crate".to_string()]).unwrap();
    let last = replica_revisions(&library);
    assert!(!last.contains_key("uid://crate"));
    for (id, revision) in &last {
        assert_eq!(after.get(id), Some(revision), "{id} was not part of the delete");
    }
}
