//! Benchmark of a client's first sync and of the server's cost of serving the feed from 0, on a library of
//! N rows written directly (`bench-library --direct --lib-name library`). Not run by default.
//!
//! ```text
//! bench-library --dir D:\dev\toybox-bench\step5\sync100k --items 100000 --lib-name library --direct
//! TOYBOX_BENCH_DATA=D:\dev\toybox-bench\step5\sync100k TOYBOX_BENCH_ITEMS=100000 [TOYBOX_BENCH_PAGE=5000] \
//!   cargo test --release -p toybox-remote --test bench_sync -- --ignored --nocapture
//! ```
//!
//! Prints `BENCH ...` lines: the server's time for the first page and for a page deep in the feed (a request
//! with `since=0` and a cursor), the client's first sync (rows per second), then a feed of 100 changed rows
//! (request, and the client taking them).

mod common;

use std::path::PathBuf;
use std::time::Instant;

use common::*;
use serde_json::json;
use toybox_core::infra::store::{bench_data, Store};
use toybox_server::config::Role;

#[test]
#[ignore = "needs a generated library: see the module comment"]
fn first_sync_and_the_feed_from_zero() {
    let dir = PathBuf::from(std::env::var("TOYBOX_BENCH_DATA").expect("TOYBOX_BENCH_DATA names a generated data folder"));
    assert!(dir.starts_with("D:\\dev\\toybox-bench") || dir.starts_with("D:/dev/toybox-bench"), "the benchmark only runs below D:\\dev\\toybox-bench");
    let items: usize = std::env::var("TOYBOX_BENCH_ITEMS").expect("TOYBOX_BENCH_ITEMS").parse().unwrap();
    let page_rows: usize = std::env::var("TOYBOX_BENCH_PAGE").ok().and_then(|v| v.parse().ok()).unwrap_or(5000);

    let server = TestServer::at(dir);
    let token = server.token(&format!("bench-sync-{}", std::process::id()), Role::Write);
    let mut world = World::new("bench-sync-a");
    let a = world.add(&server, &token, "A");
    a.connect();
    let db_id = Store::for_handle(&server.handle()).db_id();

    // The server's side: the first page, and a page far into the feed (cursor taken from a page 80% of the way).
    let start = Instant::now();
    let first = a.client().changes(0, None, page_rows, &db_id).unwrap();
    let first_secs = start.elapsed().as_secs_f64();
    assert!(!first.assets.is_empty());
    let mut page = first.clone();
    let mut pages = 1usize;
    let target = items / page_rows * 8 / 10;
    let mut deep_secs = 0.0;
    while page.more && pages < target {
        page = a.client().changes(page.next, page.after.as_deref(), page_rows, &db_id).unwrap();
        pages += 1;
    }
    if page.more {
        let start = Instant::now();
        let deep = a.client().changes(page.next, page.after.as_deref(), page_rows, &db_id).unwrap();
        deep_secs = start.elapsed().as_secs_f64();
        assert!(!deep.assets.is_empty());
    }
    eprintln!("BENCH items={items} page_rows={page_rows} server_first_page={first_secs:.3}s server_deep_page={deep_secs:.3}s");

    // The client's first sync.
    a.set_page_rows(page_rows);
    if let Some(pages) = std::env::var("TOYBOX_BENCH_PAGES").ok().and_then(|v| v.parse::<usize>().ok()) {
        // A look at the first pages only (with TOYBOX_TRACE_SYNC=1 each page prints its time).
        let start = Instant::now();
        a.sync_pages(pages).unwrap();
        eprintln!("BENCH items={items} pages={pages} secs={:.2}", start.elapsed().as_secs_f64());
        return;
    }
    let start = Instant::now();
    a.sync().unwrap();
    let sync = start.elapsed().as_secs_f64();
    let rows = Store::for_handle(&a.handle()).asset_count().unwrap();
    eprintln!("BENCH items={items} first_sync={sync:.2}s rows={rows} rows_per_sec={:.0}", rows as f64 / sync);

    // A hundred changed rows: the feed, and the client taking them.
    let ids: Vec<String> = (0..100).map(|i| format!("b{}", i * (items / 100))).collect();
    bench_data::touch_many(&Store::for_handle(&server.handle()).library_path().to_path_buf(), &ids, "edited").unwrap();
    let cursor = Store::for_handle(&a.handle()).meta("remoteRevision").unwrap().parse::<i64>().unwrap();
    let start = Instant::now();
    let changes = a.client().changes(cursor, None, 1000, &db_id).unwrap();
    let feed = start.elapsed().as_secs_f64();
    assert_eq!(changes.assets.len(), 100);
    let start = Instant::now();
    a.sync().unwrap();
    let take = start.elapsed().as_secs_f64();
    eprintln!("BENCH items={items} feed_100={feed:.3}s client_sync_100={take:.3}s");
    let report = json!({"items": items, "pageRows": page_rows, "serverFirstPageSecs": first_secs, "serverDeepPageSecs": deep_secs, "firstSyncSecs": sync, "rowsPerSec": rows as f64 / sync, "feed100Secs": feed, "clientSync100Secs": take});
    eprintln!("{report}");
    if let Ok(out) = std::env::var("TOYBOX_BENCH_OUT") {
        std::fs::write(out, report.to_string()).unwrap();
    }
}
