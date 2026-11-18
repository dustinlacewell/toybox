//! Copying and moving between local and remote libraries, in every direction.

mod common;

use std::path::Path;

use common::*;
use serde_json::{json, Value};
use toybox_core::domain::catalog_model::Catalog;
use toybox_core::infra::registry::LibraryHandle;
use toybox_core::ops::catalog::{load_catalog_inner, save_user_catalog};
use toybox_core::ops::transfer::{OnConflict, Outcome, TransferMode, TransferOptions, TransferReport};
use toybox_remote::transfer::{transfer, Side};
use toybox_server::config::Role;

const ALL: [&str; 5] = ["uid://crate", "uid://barrel", "uid://stone", "uid://click", "uid://night"];

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn options(mode: TransferMode, on_conflict: OnConflict) -> TransferOptions {
    TransferOptions { mode, on_conflict, include_clips: false }
}

fn outcome(report: &TransferReport, id: &str) -> Outcome {
    report.items.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("no line for {id}")).outcome
}

fn go(source: Side, target: Side, list: &[&str], opts: TransferOptions) -> TransferReport {
    transfer(source, target, &ids(list), opts, &|_| {}).expect("transfer")
}

/// A local library filled through the real import path, with the crate given its own pack.
fn seeded_local(world: &World, id: &str) -> LibraryHandle {
    let local = world.local(id);
    let (files, entries) = standard_content("l");
    for (rel, bytes) in &files {
        let path = local.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    toybox_core::ops::import::merge_entries(&local, entries).unwrap();
    // A real pack, as an older library would have: a transfer keeps it.
    let mut doc: Value = toybox_core::infra::store::read_manifest(&local.root).unwrap();
    for entry in doc["assets"].as_array_mut().unwrap() {
        if entry["id"] == "uid://crate" {
            entry["pack"] = json!("synty_city");
            entry["variants"][0]["pack"] = json!("synty_city");
        }
    }
    toybox_core::infra::store::write_manifest(&local.root, &doc).unwrap();
    toybox_core::ops::scan::scan_library_catalog(&local).unwrap();
    local
}

fn cached(handle: &LibraryHandle) -> Catalog {
    load_catalog_inner(handle).unwrap().unwrap()
}

fn all_files_exist(root: &Path, catalog: &Catalog) -> bool {
    let mut files = Vec::new();
    for asset in &catalog.assets {
        asset.collect_files(&mut files);
    }
    files.iter().all(|rel| root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)).is_file())
}

#[test]
fn local_to_remote_uploads_items_with_their_pack_files_and_pictures_and_keeps_favourites_here() {
    let server = TestServer::start();
    let mut world = World::new("l2r");
    let local = seeded_local(&world, "src");
    // Personal data and a picture on the source.
    let mut mine = cached(&local);
    let crate_item = mine.assets.iter_mut().find(|a| a.id == "uid://crate").unwrap();
    crate_item.user.favorite = true;
    crate_item.user.tags.push("keeper".into());
    crate_item.user.collections.push("Project".into());
    save_user_catalog(&local, mine).unwrap();
    toybox_core::ops::thumbs::save_thumb(&local, "uid://barrel", &png()).unwrap();

    let remote = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let report = go(Side::Local(&local), Side::Remote(&remote), &ALL, options(TransferMode::Copy, OnConflict::Skip));
    for id in ALL {
        assert_eq!(outcome(&report, id), Outcome::Copied, "{id}: {:?}", report.items.iter().find(|i| i.id == id));
    }
    // On the server: everything, with the pack, kept originals and provenance of the source.
    let on_server = server.catalog();
    assert_eq!(on_server.assets.len(), 5);
    let crate_on_server = on_server.assets.iter().find(|a| a.id == "uid://crate").unwrap();
    assert_eq!(crate_on_server.pack, "synty_city");
    assert_eq!(crate_on_server.category, "props");
    assert_eq!(crate_on_server.variants.len(), 1);
    assert_eq!(crate_on_server.format.as_deref(), Some("obj"));
    assert_eq!(crate_on_server.origin.as_ref().unwrap().files.len(), 1);
    // Library tags and provenance went up with the items (through the server's ingest).
    assert_eq!(crate_on_server.tags, vec!["wooden crate", "prop"]);
    assert_eq!(crate_on_server.variants[0].tags, vec!["low detail"]);
    assert_eq!(crate_on_server.user.provenance.as_ref().unwrap().source.as_deref(), Some("Crate Pack"));
    let click_on_server = on_server.assets.iter().find(|a| a.id == "uid://click").unwrap();
    assert_eq!(click_on_server.tags, vec!["click", "ui"]);
    assert_eq!(click_on_server.user.provenance.as_ref().unwrap().url.as_deref(), Some("https://example.test/clicks"));
    let manifest: Value = toybox_core::infra::store::read_manifest(&server.library_root()).unwrap();
    let click_entry = manifest["assets"].as_array().unwrap().iter().find(|e| e["id"] == "uid://click").unwrap();
    assert_eq!(click_entry["tags"], json!(["click", "ui"]));
    assert!(all_files_exist(&server.library_root(), &on_server));
    let stone = on_server.assets.iter().find(|a| a.id == "uid://stone").unwrap();
    assert_eq!(stone.variants.len(), 1);
    // Rendered pictures ride along.
    assert_eq!(std::fs::read(server.handle().thumb_path("uid://barrel")).unwrap(), png());
    // Favourites and tags did not go to the server; they are in this client's cache of the remote library.
    assert!(!crate_on_server.user.favorite && !crate_on_server.user.tags.iter().any(|t| t == "keeper"));
    let mine = cached(&remote.handle());
    let carried = mine.assets.iter().find(|a| a.id == "uid://crate").unwrap();
    assert!(carried.user.favorite && carried.user.tags.iter().any(|t| t == "keeper") && carried.user.collections == vec!["Project"]);
    assert_eq!(mine.assets.len(), 5, "the remote library shows the new items at once");
    // No upload is left lying on the server.
    assert_eq!(server.state.uploads.count(), 0);

    // Again: skipped; with replace: replaced in place, and the favourites already there stay.
    let again = go(Side::Local(&local), Side::Remote(&remote), &ALL, options(TransferMode::Copy, OnConflict::Skip));
    assert!(ALL.iter().all(|id| outcome(&again, id) == Outcome::Skipped));
    let replaced = go(Side::Local(&local), Side::Remote(&remote), &["uid://crate", "uid://click"], options(TransferMode::Copy, OnConflict::Replace));
    assert!(replaced.items.iter().all(|i| i.outcome == Outcome::Replaced));
    assert_eq!(server.catalog().assets.len(), 5, "replacing never duplicates");
    assert!(cached(&remote.handle()).assets.iter().find(|a| a.id == "uid://crate").unwrap().user.favorite);
}

#[test]
fn a_move_to_a_remote_removes_the_items_from_the_source_only_when_they_arrived() {
    let server = TestServer::start();
    let mut world = World::new("l2r-move");
    let local = seeded_local(&world, "src");
    let remote = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    // One item is already there, so it is skipped and must stay in the source.
    go(Side::Local(&local), Side::Remote(&remote), &["uid://click"], options(TransferMode::Copy, OnConflict::Skip));
    let report = go(Side::Local(&local), Side::Remote(&remote), &["uid://click", "uid://barrel", "uid://stone"], options(TransferMode::Move, OnConflict::Skip));
    assert_eq!(outcome(&report, "uid://click"), Outcome::Skipped);
    assert_eq!(outcome(&report, "uid://barrel"), Outcome::Copied);
    let removed: Vec<&str> = report.items.iter().filter(|i| i.source_removed).map(|i| i.id.as_str()).collect();
    assert_eq!(removed.len(), 2);
    let left: Vec<String> = cached(&local).assets.iter().map(|a| a.id.clone()).collect();
    assert!(left.contains(&"uid://click".to_string()), "a skipped item is never removed");
    assert!(!left.contains(&"uid://barrel".to_string()) && !left.contains(&"uid://stone".to_string()));
    assert!(server.ids().contains(&"uid://barrel".to_string()));
}

#[test]
fn remote_to_local_downloads_and_writes_items_like_a_local_copy() {
    let server = TestServer::start();
    server.seed();
    toybox_core::ops::thumbs::save_thumb(&server.handle(), "uid://crate", &png()).unwrap();
    let mut world = World::new("r2l");
    let remote = world.add_loaded(&server, &server.token("friend", Role::Read), "Friend");
    // Favourites kept on this client for the remote library follow the item.
    let mut mine = cached(&remote.handle());
    let crate_item = mine.assets.iter_mut().find(|a| a.id == "uid://crate").unwrap();
    crate_item.user.favorite = true;
    crate_item.user.tags.push("mine".into());
    save_user_catalog(&remote.handle(), mine).unwrap();
    // The remote's files are not downloaded for viewing: a transfer does not need them to be.
    assert_eq!(remote.cache_size(), 0);

    let local = world.local("dest");
    let report = go(Side::Remote(&remote), Side::Local(&local), &["uid://crate", "uid://stone", "uid://click", "uid://night"], options(TransferMode::Copy, OnConflict::Skip));
    for id in ["uid://crate", "uid://stone", "uid://click", "uid://night"] {
        assert_eq!(outcome(&report, id), Outcome::Copied, "{:?}", report.items.iter().find(|i| i.id == id));
    }
    let catalog = cached(&local);
    assert_eq!(catalog.assets.len(), 4);
    assert!(all_files_exist(&local.root, &catalog));
    let copied = catalog.assets.iter().find(|a| a.id == "uid://crate").unwrap();
    assert_eq!(copied.variants.len(), 1);
    let on_server = server.catalog();
    let original = on_server.assets.iter().find(|a| a.id == "uid://crate").unwrap();
    assert_eq!((copied.pack.as_str(), copied.category.as_str()), (original.pack.as_str(), original.category.as_str()), "pack and category are the server's");
    assert_eq!(copied.tags, vec!["wooden crate", "prop"], "library tags come down with the item");
    assert_eq!(copied.variants[0].tags, vec!["low detail"]);
    assert_eq!(copied.user.provenance.as_ref().and_then(|p| p.source.as_deref()), Some("Crate Pack"));
    assert!(!copied.user.tags.iter().any(|t| t == "prop"), "and stay library tags");
    assert!(copied.user.favorite && copied.user.tags.iter().any(|t| t == "mine"), "personal data follows between libraries on this client");
    assert_eq!(std::fs::read(local.thumb_path("uid://crate")).unwrap(), png(), "the shared picture came along");
    assert_eq!(remote.cache_size(), 0, "the transfer did not fill the remote's file cache");
    // Skip and replace behave as for local libraries.
    let again = go(Side::Remote(&remote), Side::Local(&local), &["uid://crate"], options(TransferMode::Copy, OnConflict::Skip));
    assert_eq!(outcome(&again, "uid://crate"), Outcome::Skipped);
    let replaced = go(Side::Remote(&remote), Side::Local(&local), &["uid://crate"], options(TransferMode::Copy, OnConflict::Replace));
    assert_eq!(outcome(&replaced, "uid://crate"), Outcome::Replaced);
    // An id the server does not have is a failed line, not a failed transfer.
    let missing = go(Side::Remote(&remote), Side::Local(&local), &["uid://nope", "uid://barrel"], options(TransferMode::Copy, OnConflict::Skip));
    assert_eq!(outcome(&missing, "uid://nope"), Outcome::Failed);
    assert_eq!(outcome(&missing, "uid://barrel"), Outcome::Copied);
    assert_eq!(missing.items[0].id, "uid://nope", "lines come back in the order asked");
}

#[test]
fn remote_to_remote_streams_between_two_servers_and_a_move_needs_write_on_the_source() {
    let a = TestServer::start();
    a.seed();
    let b = TestServer::start();
    let mut world = World::new("r2r");
    let read_a = world.add_loaded(&a, &a.token("reader", Role::Read), "A (read)");
    let write_a = world.add_loaded(&a, &a.token("writer", Role::Write), "A (write)");
    let write_b = world.add_loaded(&b, &b.token("writer", Role::Write), "B");

    let report = go(Side::Remote(&read_a), Side::Remote(&write_b), &ALL, options(TransferMode::Copy, OnConflict::Skip));
    assert!(ALL.iter().all(|id| outcome(&report, id) == Outcome::Copied), "{report:?}");
    assert_eq!(b.ids(), a.ids());
    assert!(all_files_exist(&b.library_root(), &b.catalog()));
    assert_eq!(a.ids().len(), 5, "a copy leaves the source alone");

    // Moving out of a library you can only read is refused, with the reason.
    let refused = transfer(Side::Remote(&read_a), Side::Remote(&write_b), &ids(&["uid://click"]), options(TransferMode::Move, OnConflict::Replace), &|_| {});
    let message = refused.unwrap_err().to_string();
    assert!(message.contains("cannot be moved out") && message.contains("read-only"), "{message}");
    assert_eq!(a.ids().len(), 5);

    // With write access on the source a move is copy, verify, delete.
    let c = TestServer::start();
    let write_c = world.add_loaded(&c, &c.token("writer", Role::Write), "C");
    let moved = go(Side::Remote(&write_a), Side::Remote(&write_c), &["uid://click", "uid://night"], options(TransferMode::Move, OnConflict::Skip));
    assert!(moved.items.iter().all(|i| i.outcome == Outcome::Copied && i.source_removed), "{moved:?}");
    assert!(!a.ids().contains(&"uid://click".to_string()));
    assert_eq!(c.ids(), vec!["uid://click", "uid://night"]);
    assert!(!cached(&write_a.handle()).assets.iter().any(|x| x.id == "uid://click"), "the source's local catalog is current");
}

#[test]
fn transfers_into_a_read_only_or_unreachable_remote_are_refused_up_front() {
    let a = TestServer::start();
    let mut b = TestServer::start();
    let mut world = World::new("refuse");
    let local = seeded_local(&world, "src");
    let read_b = world.add_loaded(&b, &b.token("reader", Role::Read), "Read-only");
    let error = transfer(Side::Local(&local), Side::Remote(&read_b), &ids(&["uid://click"]), options(TransferMode::Copy, OnConflict::Skip), &|_| {}).unwrap_err();
    assert!(error.to_string().contains("read-only"), "{error}");
    assert!(b.ids().is_empty());

    let write_b = world.add_loaded(&b, &b.token("writer", Role::Write), "Writer");
    b.shutdown();
    write_b.connect();
    let error = transfer(Side::Local(&local), Side::Remote(&write_b), &ids(&["uid://click"]), options(TransferMode::Copy, OnConflict::Skip), &|_| {}).unwrap_err();
    assert!(error.to_string().contains("offline"), "{error}");

    // The same library on both ends is not a transfer.
    let write_a = world.add_loaded(&a, &a.token("writer", Role::Write), "A");
    let error = transfer(Side::Remote(&write_a), Side::Remote(&write_a), &ids(&["uid://click"]), options(TransferMode::Copy, OnConflict::Skip), &|_| {}).unwrap_err();
    assert!(error.to_string().contains("different library"), "{error}");
}

#[test]
fn transfer_progress_counts_items_and_ends_complete() {
    let server = TestServer::start();
    let mut world = World::new("progress");
    let local = seeded_local(&world, "src");
    let remote = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let seen = std::sync::Mutex::new(Vec::new());
    transfer(Side::Local(&local), Side::Remote(&remote), &ids(&ALL), options(TransferMode::Copy, OnConflict::Skip), &|p| seen.lock().unwrap().push((p.done, p.total))).unwrap();
    let seen = seen.into_inner().unwrap();
    assert!(seen.len() >= 3);
    assert!(seen.iter().all(|(done, total)| *total == 5 && done <= total));
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "progress never goes backwards: {seen:?}");
    assert_eq!(*seen.last().unwrap(), (5, 5));
}
