//! Kept source files over the wire: a source is uploaded once, and not at all when the server stores it.

mod common;

use common::*;
use serde_json::{json, Value};
use toybox_core::domain::sources;
use toybox_core::ops::transfer::{OnConflict, TransferMode, TransferOptions};
use toybox_remote::transfer::{transfer, Side};
use toybox_remote::RemoteLibrary;
use toybox_server::config::Role;

const ARCHIVE_BYTES: usize = 1_500_000;

fn archive() -> Vec<u8> {
    (0..ARCHIVE_BYTES).map(|i| (i % 251) as u8).collect()
}

fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn stage(library: &RemoteLibrary, pack: &str, name: &str, archive: &[u8]) -> Value {
    for (rel, bytes) in [(format!("library/{pack}/materials/{name}/color.png"), png()), (format!("library/{pack}/materials/{name}/stone.sbsar"), archive.to_vec())] {
        let path = library.files_root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    json!({"id": format!("uid://{name}"), "kind": "material", "pack": pack, "category": "stone", "name": name,
        "maps": [{"role": "albedo", "file": "color.png"}],
        "origin": {"plugin": "com.example.sbsar", "files": ["stone.sbsar"], "meta": {"sbsar": "stone.sbsar", "preset": name}}})
}

fn uploaded_bytes(world: &World) -> u64 {
    // The last upload report states the bytes of that upload.
    world.progress().into_iter().filter(|p| p.operation == "upload").last().map(|p| p.total_bytes).unwrap_or(0)
}

fn stored_sources(server: &TestServer) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![server.library_root().join("library/materials/_sources")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                out.push(entry.path());
            }
        }
    }
    out
}

#[test]
fn presets_of_one_archive_upload_it_once_and_a_later_import_not_at_all() {
    let server = TestServer::start();
    let mut world = World::new("sources-import");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let archive = archive();
    let entries: Vec<Value> = ["One", "Two", "Three"].iter().enumerate().map(|(n, name)| stage(&library, &format!("_import_s{n}"), name, &archive)).collect();
    let outcome = library.import(entries).unwrap();
    assert_eq!(outcome.imported.len(), 3, "{:?}", outcome.errors);
    let sent = uploaded_bytes(&world);
    assert!(sent >= ARCHIVE_BYTES as u64 && sent < 2 * ARCHIVE_BYTES as u64, "one copy of the archive went over the wire, not three: {sent} bytes");
    let stored = stored_sources(&server);
    assert_eq!(stored.len(), 1, "{stored:?}");
    assert_eq!(std::fs::read(&stored[0]).unwrap(), archive);
    let shared = sources::shared_path("materials", &hash(&archive), "stone.sbsar");
    for asset in &outcome.catalog.assets {
        assert_eq!(asset.origin.as_ref().unwrap().files, vec![shared.clone()], "{}", asset.name);
    }

    // Another preset of the same archive: the server has it, so it is not sent.
    let before = world.progress().len();
    let outcome = library.import(vec![stage(&library, "_import_s9", "Four", &archive)]).unwrap();
    assert_eq!(outcome.imported, vec!["uid://Four"], "{:?}", outcome.errors);
    let events = world.progress();
    let last = events[before..].iter().filter(|p| p.operation == "upload").last().unwrap();
    assert!(last.total_bytes < 100_000, "only the maps were sent: {} bytes", last.total_bytes);
    assert_eq!(stored_sources(&server).len(), 1);
    assert_eq!(server.ids().len(), 4);
    assert_eq!(server.state.uploads.count(), 0);
}

#[test]
fn a_transfer_to_a_server_sends_a_source_once_and_skips_one_the_server_has() {
    let server = TestServer::start();
    let mut world = World::new("sources-transfer");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let local = world.local("here");
    let archive = archive();
    for (n, name) in ["A", "B", "C"].iter().enumerate() {
        for (rel, bytes) in [(format!("library/_import_l{n}/materials/{name}/color.png"), png()), (format!("library/_import_l{n}/materials/{name}/stone.sbsar"), archive.clone())] {
            let path = local.root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }
    let entries: Vec<Value> = ["A", "B", "C"]
        .iter()
        .enumerate()
        .map(|(n, name)| json!({"id": format!("uid://{name}"), "kind": "material", "pack": format!("_import_l{n}"), "category": "stone", "name": name,
            "maps": [{"role": "albedo", "file": "color.png"}], "origin": {"plugin": "x", "files": ["stone.sbsar"], "meta": {"sbsar": "stone.sbsar"}}}))
        .collect();
    toybox_core::ops::import::merge_entries(&local, entries).unwrap();
    let options = TransferOptions { mode: TransferMode::Copy, on_conflict: OnConflict::Skip, include_clips: false };
    let ids = |list: &[&str]| list.iter().map(|id| id.to_string()).collect::<Vec<_>>();

    let report = transfer(Side::Local(&local), Side::Remote(&library), &ids(&["uid://A", "uid://B"]), options, &|_| {}).unwrap();
    assert!(report.items.iter().all(|item| item.reason.is_none()), "{report:?}");
    let sent = uploaded_bytes(&world);
    assert!(sent >= ARCHIVE_BYTES as u64 && sent < 2 * ARCHIVE_BYTES as u64, "once: {sent}");
    assert_eq!(stored_sources(&server).len(), 1);

    let before = world.progress().len();
    let report = transfer(Side::Local(&local), Side::Remote(&library), &ids(&["uid://C"]), options, &|_| {}).unwrap();
    assert!(report.items.iter().all(|item| item.reason.is_none()), "{report:?}");
    let events = world.progress();
    let last = events[before..].iter().filter(|p| p.operation == "upload").last().unwrap();
    assert!(last.total_bytes < 100_000, "the server had the archive: {} bytes", last.total_bytes);
    assert_eq!(stored_sources(&server).len(), 1);
    assert_eq!(server.ids(), vec!["uid://A", "uid://B", "uid://C"]);

    // And down again: the archive is fetched once for items that share it.
    let back = world.local("back");
    let report = transfer(Side::Remote(&library), Side::Local(&back), &ids(&["uid://A", "uid://B", "uid://C"]), options, &|_| {}).unwrap();
    assert!(report.items.iter().all(|item| item.reason.is_none()), "{report:?}");
    let shared = sources::shared_path("materials", &hash(&archive), "stone.sbsar");
    assert!(back.root.join(&shared).is_file());
    let count = std::fs::read_dir(back.root.join("library/materials/_sources")).unwrap().count();
    assert_eq!(count, 1);
}
