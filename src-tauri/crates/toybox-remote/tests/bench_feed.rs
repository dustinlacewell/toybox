//! Benchmark of the wire: what one metadata edit of 100 items costs the server,
//! the feed and a following client, on a library of N items. Not run by default.
//!
//! ```text
//! bench-library --dir D:\dev\toybox-bench\step3\n20000 --items 20000 --lib-name library --generate-only
//! TOYBOX_BENCH_DATA=D:\dev\toybox-bench\step3\n20000 TOYBOX_BENCH_ITEMS=20000 [TOYBOX_BENCH_LEGACY=1] \
//!   cargo test --release -p toybox-remote --test bench_feed -- --ignored --nocapture
//! ```
//!
//! The generated folder is served as a server data folder (a `server.json` is added) and is
//! left in place. `TOYBOX_BENCH_LEGACY=1` also times what a version 1 client costs, built from
//! the same pieces the version 1 server and client used per change (a whole catalog read,
//! stripped and serialised; parsed, merged and saved on the other side).

mod common;

use std::path::PathBuf;
use std::time::Instant;

use common::*;
use serde_json::json;
use toybox_core::domain::catalog_model::Catalog;
use toybox_core::infra::store::Store;
use toybox_core::ops::metadata::MetadataEdit;
use toybox_remote::client::CatalogReply;
use toybox_server::config::Role;

fn spread(n: usize, count: usize, offset: usize) -> Vec<usize> {
    let step = (n / count).max(1);
    (0..count).map(|i| (i * step + offset) % n).collect()
}

fn edits(n: usize, round: usize) -> Vec<MetadataEdit> {
    spread(n, 100, 7 + round * 13)
        .into_iter()
        .map(|i| MetadataEdit { id: format!("uid://b{i}"), tags: Some(vec![format!("bench-{}-round{round}", std::process::id()), format!("e{}", i % 7)]), provenance: None })
        .collect()
}

#[test]
#[ignore = "needs a generated library: see the module comment"]
fn one_edit_of_a_hundred_items_costs_the_same_on_any_library() {
    let dir = PathBuf::from(std::env::var("TOYBOX_BENCH_DATA").expect("TOYBOX_BENCH_DATA names a generated data folder"));
    assert!(dir.starts_with("D:\\dev\\toybox-bench") || dir.starts_with("D:/dev/toybox-bench"), "the benchmark only runs below D:\\dev\\toybox-bench");
    let items: usize = std::env::var("TOYBOX_BENCH_ITEMS").expect("TOYBOX_BENCH_ITEMS").parse().unwrap();
    let legacy = std::env::var("TOYBOX_BENCH_LEGACY").is_ok_and(|v| v == "1");

    let server = TestServer::at(dir);
    let token = server.token(&format!("bench-writer-{}", std::process::id()), Role::Write);
    let (mut world_a, mut world_b) = (World::new("bench-a"), World::new("bench-b"));
    let a = world_a.add(&server, &token, "A");
    let b = world_b.add(&server, &token, "B");
    a.connect();

    // The first sync is a full paged download: O(N) by nature, and done once.
    let start = Instant::now();
    a.sync().unwrap();
    let first_sync = start.elapsed().as_secs_f64();
    eprintln!("BENCH items={items} first_full_sync={first_sync:.3}s");
    let db_id = Store::for_handle(&server.handle()).db_id();

    let mut rounds = Vec::new();
    for round in 0..3 {
        let cursor = Store::for_handle(&a.handle()).meta("remoteRevision").unwrap().parse::<i64>().unwrap();
        let wanted = edits(items, round);

        // (a) the server handles the edit end to end (HTTP in, edit, reply with the 100 rows out).
        let start = Instant::now();
        let reply = b.client().update_metadata(&wanted).unwrap();
        let edit = start.elapsed().as_secs_f64();
        assert_eq!(reply.changes.as_ref().map(|page| page.assets.len()), Some(100));

        // (b) the feed for those 100.
        let start = Instant::now();
        let page = a.client().changes(cursor, None, 1000, &db_id).unwrap();
        let feed = start.elapsed().as_secs_f64();
        assert_eq!(page.assets.len(), 100);

        // (c) a client takes them: one request plus the row-level apply.
        let start = Instant::now();
        a.sync().unwrap();
        let sync = start.elapsed().as_secs_f64();
        assert_eq!(Store::for_handle(&a.handle()).meta("remoteRevision").unwrap().parse::<i64>().unwrap(), page.revision);

        // And its own edit: the server's work plus applying the reply directly.
        let own = edits(items, round + 50);
        let start = Instant::now();
        a.update_metadata(&own).unwrap();
        let own_secs = start.elapsed().as_secs_f64();

        eprintln!("BENCH items={items} round={round} edit_100={edit:.3}s feed_100={feed:.3}s client_sync_100={sync:.3}s (apply ~ {:.3}s) own_edit_with_apply={own_secs:.3}s", (sync - feed).max(0.0));
        rounds.push(json!({"editSecs": edit, "feedSecs": feed, "clientSyncSecs": sync, "applySecs": (sync - feed).max(0.0), "ownEditSecs": own_secs}));
    }

    let mut report = json!({"items": items, "firstFullSyncSecs": first_sync, "rounds": rounds});
    if legacy {
        // What version 1 did for every change, from the same pieces.
        let mut world_c = World::new("bench-c");
        let c = world_c.add(&server, &token, "C");
        c.connect();
        c.sync().unwrap();
        b.client().update_metadata(&edits(items, 90)).unwrap();
        let start = Instant::now();
        let CatalogReply::Fresh { body, .. } = a.client().catalog(None).unwrap() else { panic!("a catalog") };
        let server_secs = start.elapsed().as_secs_f64();
        let store = Store::for_handle(&c.handle());
        let start = Instant::now();
        let fresh: Catalog = serde_json::from_slice(&body).unwrap();
        let prior = store.load_catalog().unwrap().unwrap();
        let merged = toybox_core::domain::merge::merge_preserving_user(&prior, fresh);
        store.save_catalog(&merged).unwrap();
        let client_secs = start.elapsed().as_secs_f64();
        eprintln!("BENCH items={items} v1_server_catalog_build_and_send={server_secs:.3}s ({} MB) v1_client_parse_merge_save={client_secs:.3}s", body.len() / 1_000_000);
        report["version1"] = json!({"catalogBytes": body.len(), "serverBuildAndSendSecs": server_secs, "clientParseMergeSaveSecs": client_secs});
    }
    eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());
}
