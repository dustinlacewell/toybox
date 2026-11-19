//! Following the server's revision feed: a replica that is kept up to date by
//! small pages ends up the same as one downloaded in full, personal data is
//! never lost on the way, and the reply to this client's own write is applied
//! as it comes.

mod common;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::atomic::Ordering;

use common::*;
use rusqlite::Connection;
use serde_json::{json, Value};
use toybox_core::infra::registry::LibraryEntry;
use toybox_core::infra::store::Store;
use toybox_core::ops::catalog::{load_catalog_inner, save_user_catalog};
use toybox_core::ops::transfer::{OnConflict, TransferMode, TransferOptions};
use toybox_remote::transfer::{transfer, Side};
use toybox_remote::{RemoteError, RemoteLibrary};
use toybox_server::config::Role;

/// The replica's assets by id (library data and personal data), and its rigs by id.
fn replica(library: &RemoteLibrary) -> (BTreeMap<String, Value>, BTreeMap<String, Value>) {
    let catalog = Store::for_handle(&library.handle()).load_catalog().unwrap().expect("a replica");
    let assets = catalog.assets.iter().map(|asset| (asset.id.clone(), serde_json::to_value(asset).unwrap())).collect();
    let rigs = catalog.rigs.iter().map(|rig| (rig.id.clone(), serde_json::to_value(rig).unwrap())).collect();
    (assets, rigs)
}

/// The replica's asset rows with the revision that last wrote them.
fn revisions(library: &RemoteLibrary) -> BTreeMap<String, i64> {
    let connection = Connection::open(library.handle().library_db_path()).unwrap();
    let mut statement = connection.prepare("SELECT id, revision FROM assets").unwrap();
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).unwrap();
    rows.map(Result::unwrap).collect()
}

fn rows_written_since(before: &BTreeMap<String, i64>, library: &RemoteLibrary) -> Vec<String> {
    revisions(library).into_iter().filter(|(id, revision)| before.get(id) != Some(revision)).map(|(id, _)| id).collect()
}

fn meta(library: &RemoteLibrary, key: &str) -> Option<String> {
    Store::for_handle(&library.handle()).meta(key)
}

fn server_revision(server: &TestServer) -> i64 {
    Store::for_handle(&server.handle()).status().unwrap().revision
}

fn stage(library: &RemoteLibrary, files: &[(&str, Vec<u8>)]) {
    for (rel, bytes) in files {
        let path = library.files_root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}

fn beep(pack: &str, id: &str) -> (Vec<(String, Vec<u8>)>, Vec<Value>) {
    (
        vec![(format!("library/{pack}/sounds/{id}.wav"), wav())],
        vec![json!({"id": format!("uid://{id}"), "kind": "audio", "pack": pack, "category": "sounds", "file": format!("{id}.wav")})],
    )
}

fn import_beep(library: &RemoteLibrary, pack: &str, id: &str) {
    let (files, entries) = beep(pack, id);
    let files: Vec<(&str, Vec<u8>)> = files.iter().map(|(rel, bytes)| (rel.as_str(), bytes.clone())).collect();
    stage(library, &files);
    library.import(entries).unwrap();
}

fn edits(value: Value) -> Vec<toybox_core::ops::metadata::MetadataEdit> {
    serde_json::from_value(value).unwrap()
}

#[test]
fn a_replica_followed_by_small_pages_equals_one_downloaded_in_full_across_imports_edits_deletes_and_transfers() {
    let server = TestServer::start();
    server.seed();
    let token = server.token("w", Role::Write);
    let (mut world_a, mut world_b, mut world_c) = (World::new("sync-a"), World::new("sync-b"), World::new("sync-c"));
    let a = world_a.add(&server, &token, "A");
    a.set_page_rows(2);
    a.connect();
    a.refresh_catalog(false).unwrap();
    assert_eq!(a.stats.catalog_downloads.load(Ordering::Relaxed), 1, "three pages of two rows are one download");
    let b = world_b.add_loaded(&server, &token, "B");
    assert_eq!(replica(&a), replica(&b));
    assert_eq!(replica(&a).0.len(), 5);
    assert_eq!(meta(&a, "remoteRevision"), Some(server_revision(&server).to_string()));
    assert_eq!(meta(&a, "remoteAfter").unwrap_or_default(), "");
    assert_eq!(meta(&a, "remoteDbId").as_deref(), Some(Store::for_handle(&server.handle()).db_id().as_str()));

    // Nothing happened: nothing is downloaded or written.
    let before = revisions(&a);
    let stamp = Store::for_handle(&a.handle()).stamp().unwrap();
    a.refresh_catalog(false).unwrap();
    assert_eq!(a.stats.catalog_downloads.load(Ordering::Relaxed), 1);
    assert_eq!(Store::for_handle(&a.handle()).stamp().unwrap(), stamp);

    // Another client imports.
    import_beep(&b, "_import_x2", "beep");
    let downloads = a.stats.catalog_downloads.load(Ordering::Relaxed);
    a.refresh_catalog(false).unwrap();
    assert_eq!(a.stats.catalog_downloads.load(Ordering::Relaxed), downloads + 1);
    assert_eq!(rows_written_since(&before, &a), ["uid://beep"], "only the new row was written");
    let c = world_c.add_loaded(&server, &token, "C");
    assert_eq!(replica(&a), replica(&c));

    // It edits, then deletes.
    let before = revisions(&a);
    b.update_metadata(&edits(json!([{"id": "uid://click", "tags": ["Coin"]}, {"id": "uid://crate_lod", "tags": ["lod2"]}]))).unwrap();
    a.refresh_catalog(false).unwrap();
    assert_eq!(rows_written_since(&before, &a), ["uid://click", "uid://crate"], "the edited rows, a variant inside its parent");
    let before = revisions(&a);
    b.delete_assets(&["uid://barrel".to_string()]).unwrap();
    a.refresh_catalog(false).unwrap();
    assert_eq!(rows_written_since(&before, &a), Vec::<String>::new(), "a removal writes no other row");
    assert!(!replica(&a).0.contains_key("uid://barrel"));
    let c = world_c.add_loaded(&server, &token, "C2");
    assert_eq!(replica(&a), replica(&c));

    // It copies items in from a library of its own: an ingest.
    let local = world_b.local("mine");
    let (files, entries) = beep("_import_loc", "ding");
    for (rel, bytes) in &files {
        let path = local.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    toybox_core::ops::import::merge_entries(&local, entries).unwrap();
    let before = revisions(&a);
    let options = TransferOptions { mode: TransferMode::Copy, on_conflict: OnConflict::Skip, include_clips: false };
    let report = transfer(Side::Local(&local), Side::Remote(&b), &["uid://ding".to_string()], options, &|_| {}).unwrap();
    assert_eq!(report.items.len(), 1);
    a.refresh_catalog(false).unwrap();
    assert_eq!(rows_written_since(&before, &a), ["uid://ding"]);
    let c = world_c.add_loaded(&server, &token, "C3");
    assert_eq!(replica(&a), replica(&c), "rows and rigs");
    assert_eq!(meta(&a, "remoteRevision"), Some(server_revision(&server).to_string()));
}

#[test]
fn personal_data_survives_incremental_syncs_resets_and_forced_downloads() {
    let server = TestServer::start();
    server.seed();
    let token = server.token("w", Role::Write);
    let (mut world_a, mut world_b) = (World::new("keep-a"), World::new("keep-b"));
    let a = world_a.add_loaded(&server, &token, "A");
    let b = world_b.add_loaded(&server, &token, "B");
    let mut mine = load_catalog_inner(&a.handle()).unwrap().unwrap();
    {
        let item = mine.assets.iter_mut().find(|asset| asset.id == "uid://crate").unwrap();
        item.user.favorite = true;
        item.user.tags.push("mine".into());
        item.user.collections.push("Project".into());
        item.user.preview_asset_id = Some("uid://barrel".into());
        item.variants[0].user.favorite = true;
        let click = mine.assets.iter_mut().find(|asset| asset.id == "uid://click").unwrap();
        click.user.collections.push("Sounds".into());
    }
    save_user_catalog(&a.handle(), mine).unwrap();
    let kept = |library: &RemoteLibrary| -> Value {
        let assets = replica(library).0;
        let crate_item = &assets["uid://crate"];
        json!({
            "favorite": crate_item["user"]["favorite"], "collections": crate_item["user"]["collections"], "preview": crate_item["user"]["previewAssetId"],
            "mine": crate_item["user"]["tags"].as_array().unwrap().contains(&json!("mine")), "variant": crate_item["variants"][0]["user"]["favorite"],
            "click": assets["uid://click"]["user"]["collections"],
        })
    };
    let expected = kept(&a);
    assert_eq!(expected["favorite"], true);

    // The server's data about the same items changes (an edit by someone else): the rows are rewritten, the personal data stays.
    b.update_metadata(&edits(json!([{"id": "uid://crate", "tags": ["Footstep"], "provenance": {"source": "Better"}}]))).unwrap();
    import_beep(&b, "_import_x2", "beep");
    a.refresh_catalog(false).unwrap();
    assert_eq!(kept(&a), expected);
    assert_eq!(replica(&a).0["uid://crate"]["tags"], json!(["footstep"]));

    // The server forgets deletions older than a cursor: the replica starts from 0 and keeps its personal data.
    toybox_core::ops::delete::delete_from_library(&server.handle().root, &server.handle().cache_path, &["uid://beep".to_string()]).unwrap();
    toybox_core::ops::delete::delete_from_library(&server.handle().root, &server.handle().cache_path, &["uid://night".to_string()]).unwrap();
    Store::for_handle(&server.handle()).prune_changes(0).unwrap();
    let downloads = a.stats.catalog_downloads.load(Ordering::Relaxed);
    a.refresh_catalog(false).unwrap();
    assert_eq!(a.stats.catalog_downloads.load(Ordering::Relaxed), downloads + 1);
    let after = replica(&a).0;
    assert!(!after.contains_key("uid://night") && !after.contains_key("uid://beep"), "the reset removed what the server no longer has");
    assert_eq!(kept(&a), expected);

    // The library the server serves is another database: start again as well.
    Connection::open(server.handle().library_db_path()).unwrap().execute("UPDATE meta SET value='replaced' WHERE key='dbId'", []).unwrap();
    a.refresh_catalog(false).unwrap();
    assert_eq!(meta(&a, "remoteDbId").as_deref(), Some("replaced"));
    assert_eq!(kept(&a), expected);

    // A forced refresh downloads everything again.
    let downloads = a.stats.catalog_downloads.load(Ordering::Relaxed);
    a.refresh_catalog(true).unwrap();
    assert_eq!(a.stats.catalog_downloads.load(Ordering::Relaxed), downloads + 1);
    assert_eq!(kept(&a), expected);
    // And what is deleted for good takes its personal data with it.
    a.delete_assets(&["uid://crate".to_string()]).unwrap();
    assert!(!replica(&a).0.contains_key("uid://crate"));
}

#[test]
fn a_replica_from_before_the_feed_upgrades_with_a_full_download_and_keeps_personal_data() {
    let server = TestServer::start();
    server.seed();
    let token = server.token("w", Role::Write);
    let mut world = World::new("upgrade");
    let a = world.add_loaded(&server, &token, "A");
    let mut mine = load_catalog_inner(&a.handle()).unwrap().unwrap();
    mine.assets.iter_mut().find(|asset| asset.id == "uid://crate").unwrap().user.favorite = true;
    save_user_catalog(&a.handle(), mine).unwrap();
    let before = replica(&a);
    // The state an older version left: rows and favourites, no cursor.
    let connection = Connection::open(a.handle().library_db_path()).unwrap();
    connection.execute("DELETE FROM meta WHERE key LIKE 'remote%'", []).unwrap();
    drop(connection);
    let downloads = a.stats.catalog_downloads.load(Ordering::Relaxed);
    a.set_page_rows(2);
    a.refresh_catalog(false).unwrap();
    assert_eq!(a.stats.catalog_downloads.load(Ordering::Relaxed), downloads + 1);
    assert_eq!(replica(&a), before, "the same rows, the same favourite");
    assert_eq!(meta(&a, "remoteRevision"), Some(server_revision(&server).to_string()));
}

#[test]
fn an_interrupted_download_goes_on_from_the_page_it_reached() {
    let server = TestServer::start();
    server.seed();
    let token = server.token("w", Role::Write);
    let (mut world, mut other) = (World::new("resume"), World::new("resume-c"));
    let a = world.add(&server, &token, "A");
    a.set_page_rows(2);
    a.connect();
    // Two pages of the three arrive, then the download stops.
    a.sync_pages(2).unwrap();
    assert_eq!(replica(&a).0.len(), 4);
    assert!(Store::for_handle(&a.handle()).catalog_exists());
    let cursor = meta(&a, "remoteRevision");
    assert!(cursor.is_some() && meta(&a, "remoteAfter").is_some_and(|after| !after.is_empty()), "the cursor sits inside the revision");
    // The next sync asks for the rest only.
    a.refresh_catalog(false).unwrap();
    assert_eq!(a.stats.catalog_downloads.load(Ordering::Relaxed), 2);
    assert_eq!(replica(&a).0.len(), 5);
    let c = other.add_loaded(&server, &token, "C");
    assert_eq!(replica(&a), replica(&c));
    assert_eq!(meta(&a, "remoteAfter").unwrap_or_default(), "");
}

/// What a replica's database holds besides the assets: its schema objects, the counts, and what a search finds.
fn structure(library: &RemoteLibrary) -> (Vec<(String, String)>, Vec<(String, String, String, i64)>, BTreeMap<String, Vec<String>>) {
    let connection = Connection::open(library.handle().library_db_path()).unwrap();
    let objects = connection
        .prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND name NOT LIKE 'asset_search_%' ORDER BY type, name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let facets = connection
        .prepare("SELECT type, field, value, n FROM facets ORDER BY type, field, value")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut searches = BTreeMap::new();
    for needle in ["beep", "crate", "oo", "wav", "ou", "footstep"] {
        let mut statement = connection.prepare("SELECT a.id FROM asset_search s JOIN assets a ON a.rowid = s.rowid WHERE asset_search MATCH ?1 ORDER BY a.id").unwrap();
        let found: Vec<String> = statement.query_map([format!("\"{needle}\"")], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
        searches.insert(needle.to_string(), found);
    }
    (objects, facets, searches)
}

#[test]
fn a_bulk_first_download_equals_one_that_follows_the_feed_and_later_syncs_are_unaffected() {
    let server = TestServer::start();
    server.seed();
    let token = server.token("w", Role::Write);
    let (mut writer_world, mut a_world, mut b_world, mut c_world) = (World::new("bulk-w"), World::new("bulk-a"), World::new("bulk-b"), World::new("bulk-c"));
    let writer = writer_world.add_loaded(&server, &token, "W");
    for i in 0..7 {
        import_beep(&writer, "_import_bulk", &format!("beep{i}"));
    }
    writer.update_metadata(&edits(json!([{"id": "uid://crate", "tags": ["Footstep", "wood"]}, {"id": "uid://beep3", "tags": ["loud"]}]))).unwrap();

    // A takes the library in small pages (bulk mode: rows first, indexes at the end); B in one page.
    let a = a_world.add(&server, &token, "A");
    a.set_page_rows(3);
    a.connect();
    a.sync().unwrap();
    let b = b_world.add(&server, &token, "B");
    b.connect();
    b.sync().unwrap();
    assert!(!Store::for_handle(&a.handle()).is_bulk(), "the download ended, so the replica is whole");
    assert!(replica(&a).0.len() > 8);
    assert_eq!(replica(&a), replica(&b));
    assert_eq!(structure(&a), structure(&b), "the same indexes, triggers, counts and search rows");
    let integrity: String = Connection::open(a.handle().library_db_path()).unwrap().query_row("PRAGMA integrity_check", [], |row| row.get(0)).unwrap();
    assert_eq!(integrity, "ok");

    // A favourite set here survives a forced download, which is a bulk one again.
    let mut mine = load_catalog_inner(&a.handle()).unwrap().unwrap();
    mine.assets.iter_mut().find(|asset| asset.id == "uid://crate").unwrap().user.favorite = true;
    save_user_catalog(&a.handle(), mine).unwrap();
    a.refresh_catalog(true).unwrap();
    assert_eq!(replica(&a).0["uid://crate"]["user"]["favorite"], json!(true));
    assert!(!Store::for_handle(&a.handle()).is_bulk());
    assert_eq!(structure(&a), structure(&b));

    // Later changes arrive row by row on the bulk-built replica exactly as on the other one.
    writer.update_metadata(&edits(json!([{"id": "uid://beep1", "tags": ["Changed"]}]))).unwrap();
    import_beep(&writer, "_import_bulk2", "late");
    toybox_core::ops::delete::delete_from_library(&server.handle().root, &server.handle().cache_path, &["uid://beep5".to_string()]).unwrap();
    a.sync().unwrap();
    b.sync().unwrap();
    let (mut ra, rb) = (replica(&a), replica(&b));
    ra.0.get_mut("uid://crate").unwrap()["user"]["favorite"] = json!(false);
    assert_eq!(ra, rb);
    assert_eq!(structure(&a), structure(&b));

    // An interrupted first download is not shown as a library, goes on from its cursor, and ends whole.
    let c = c_world.add(&server, &token, "C");
    c.set_page_rows(2);
    c.connect();
    c.sync_pages(2).unwrap();
    assert!(Store::for_handle(&c.handle()).is_bulk());
    let sources = [toybox_core::infra::store::browse::Source { id: "c".into(), library: c.handle().library_db_path(), user: c.handle().user_db_path(), rank: 0 }];
    assert_eq!(toybox_core::infra::store::browse::Session::open(&sources, &[]).unwrap().library_ids().len(), 0, "half a download is not listed");
    c.sync().unwrap();
    assert!(!Store::for_handle(&c.handle()).is_bulk());
    assert_eq!(toybox_core::infra::store::browse::Session::open(&sources, &[]).unwrap().library_ids().len(), 1);
    assert_eq!(replica(&c), rb);
    assert_eq!(structure(&c), structure(&b));
}

#[test]
fn the_reply_to_an_own_write_is_applied_as_it_comes_and_equals_a_pull() {
    let server = TestServer::start();
    server.seed();
    let token = server.token("w", Role::Write);
    let (mut world_a, mut world_b, mut world_c) = (World::new("own-a"), World::new("own-b"), World::new("own-c"));
    let a = world_a.add_loaded(&server, &token, "A");
    let b = world_b.add_loaded(&server, &token, "B");

    // Own write while current: applied directly, the cursor moves, nothing is pulled.
    let downloads = a.stats.catalog_downloads.load(Ordering::Relaxed);
    a.update_metadata(&edits(json!([{"id": "uid://click", "tags": ["Coin"]}]))).unwrap();
    import_beep(&a, "_import_own", "own1");
    assert_eq!(a.stats.catalog_downloads.load(Ordering::Relaxed), downloads, "no download for what the replies carried");
    assert_eq!(meta(&a, "remoteRevision"), Some(server_revision(&server).to_string()));
    let c = world_c.add_loaded(&server, &token, "C");
    assert_eq!(replica(&a), replica(&c), "the replica built from replies equals a pulled one");

    // Somebody else wrote in between: the reply is not applied alone, the feed is followed, and nothing is missed.
    import_beep(&b, "_import_theirs", "theirs");
    a.update_metadata(&edits(json!([{"id": "uid://night", "tags": ["dark"]}]))).unwrap();
    assert_eq!(a.stats.catalog_downloads.load(Ordering::Relaxed), downloads + 1);
    let all = replica(&a).0;
    assert!(all.contains_key("uid://theirs") && all["uid://night"]["tags"] == json!(["dark"]));
    assert_eq!(meta(&a, "remoteRevision"), Some(server_revision(&server).to_string()));
    let c = world_c.add_loaded(&server, &token, "C2");
    assert_eq!(replica(&a), replica(&c));
    // The same for a delete.
    a.delete_assets(&["uid://theirs".to_string(), "uid://own1".to_string()]).unwrap();
    assert_eq!(a.stats.catalog_downloads.load(Ordering::Relaxed), downloads + 1);
    assert_eq!(replica(&a).0.len(), 5);
    let c = world_c.add_loaded(&server, &token, "C3");
    assert_eq!(replica(&a), replica(&c));
}

#[test]
fn fingerprints_the_server_adds_later_reach_the_replica_as_changes() {
    let server = TestServer::start();
    server.seed();
    let token = server.token("w", Role::Write);
    let mut world = World::new("hashes");
    let a = world.add_loaded(&server, &token, "A");
    assert!(replica(&a).0.values().all(|asset| asset["contentHash"].is_string()));
    toybox_core::infra::store::hashes::update(&server.handle().library_db_path(), |map| map.clear()).unwrap();
    a.refresh_catalog(false).unwrap();
    assert!(replica(&a).0.values().all(|asset| asset.get("contentHash").is_none_or(Value::is_null)));
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let report = runtime.block_on(server.state.library.fingerprint_pass()).unwrap();
    assert!(report.hashed >= 5, "{report:?}");
    let downloads = a.stats.catalog_downloads.load(Ordering::Relaxed);
    a.refresh_catalog(false).unwrap();
    assert_eq!(a.stats.catalog_downloads.load(Ordering::Relaxed), downloads + 1);
    assert!(replica(&a).0.values().all(|asset| asset["contentHash"].is_string()));
    assert!(replica(&a).0["uid://crate"]["variants"][0]["contentHash"].is_string());
}

/// A server of the first API version: it answers `/info` and nothing else.
fn old_server() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut seen = Vec::new();
            let mut buffer = [0u8; 1024];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => seen.extend_from_slice(&buffer[..count]),
                }
            }
            let body = r#"{"name":"Old server","serverVersion":"0.3.0","apiVersion":1,"role":"write","revision":"0123456789abcdef"}"#;
            let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    format!("http://{address}")
}

#[test]
fn a_server_of_the_first_version_is_refused_with_a_clear_message() {
    let url = old_server();
    let error = toybox_remote::probe(&url, "tbx_whatever").unwrap_err();
    assert!(matches!(error, RemoteError::ServerTooOld { found: 1, needed: 2, .. }), "{error:?}");
    assert!(error.to_string().contains("too old") && error.to_string().contains("version 1"), "{error}");
    let mut world = World::new("old");
    let entry: LibraryEntry = world.settings.add_remote_library(&url, "Old", "old-lib".into()).unwrap();
    world.hub.store_token("old-lib", "tbx_whatever").unwrap();
    let library = world.hub.get(&entry).unwrap();
    let status = library.connect();
    assert!(status.offline && status.offline_reason.unwrap().contains("too old"));
    assert_eq!(status.compat.map(|c| (c.state, c.server_version)), Some(("server_too_old".to_string(), "0.3.0".to_string())));
    let error = library.refresh_catalog(false).unwrap_err();
    assert!(error.to_string().contains("too old"), "{error}");
}

/// A server that answers every request with one fixed reply and keeps what it was asked.
fn fixed_server(reply_status: &str, body: &'static str) -> (String, std::sync::mpsc::Receiver<String>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let status = reply_status.to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut seen = Vec::new();
            let mut buffer = [0u8; 1024];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => seen.extend_from_slice(&buffer[..count]),
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&seen).to_string());
            let reply = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (format!("http://{address}"), receiver)
}

#[test]
fn a_client_older_than_the_window_is_told_so_in_plain_words_and_names_itself() {
    let (url, requests) = fixed_server(
        "426 Upgrade Required",
        r#"{"error":{"code":"client_too_old","message":"x"},"serverVersion":"9.1.0","apiVersion":9,"minApiVersion":8,"minClientVersion":"9.0.0"}"#,
    );
    let error = toybox_remote::probe(&url, "tbx_whatever").unwrap_err();
    assert_eq!(error, RemoteError::ClientTooOld { server_version: "9.1.0".into(), min_client_version: "9.0.0".into() });
    let text = error.to_string();
    assert!(text.contains("runs Toybox 9.1.0") && text.contains("Update Toybox to 9.0.0 or newer"), "{text}");
    // Every request says who is asking.
    let seen = requests.recv().unwrap().to_ascii_lowercase();
    assert!(seen.contains(&format!("x-toybox-client: toybox/{}", env!("CARGO_PKG_VERSION"))), "{seen}");
    assert!(seen.contains("x-toybox-api: 2"));
    // The library shows offline with that sentence, and remembers why.
    let mut world = World::new("too-new-server");
    let entry: LibraryEntry = world.settings.add_remote_library(&url, "Future", "future-lib".into()).unwrap();
    world.hub.store_token("future-lib", "tbx_whatever").unwrap();
    let library = world.hub.get(&entry).unwrap();
    let status = library.connect();
    assert!(status.offline);
    let compat = status.compat.expect("the versions are remembered");
    assert_eq!((compat.state.as_str(), compat.min_client_version.as_deref()), ("client_too_old", Some("9.0.0")));
    assert!(status.offline_reason.unwrap().contains("Update Toybox to 9.0.0"));
}

#[test]
fn a_server_of_the_same_release_fits_and_says_what_it_is() {
    let server = TestServer::start();
    let admin = server.admin.clone();
    let probe = toybox_remote::probe(&server.url, &admin).unwrap();
    assert_eq!(probe.notice, None, "same version, nothing to say");
    let mut world = World::new("fits");
    let library = world.add(&server, &admin, "Fits");
    let status = library.connect();
    let compat = status.compat.expect("compat after connecting");
    assert_eq!(compat.state, "ok");
    assert_eq!(compat.server_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(compat.min_api_version, Some(1));
}

#[test]
fn a_server_inside_the_window_but_of_another_version_gives_a_notice_not_an_error() {
    let (url, _) = fixed_server("200 OK", r#"{"name":"Older","serverVersion":"0.1.0","apiVersion":2,"minApiVersion":1,"minClientVersion":"0.1.0","role":"read","revision":1}"#);
    let probe = toybox_remote::probe(&url, "tbx_whatever").unwrap();
    let notice = probe.notice.expect("a notice");
    assert!(notice.contains("0.1.0") && notice.contains("toybox-server update"), "{notice}");
    assert_eq!(probe.server_name, "Older");
}
