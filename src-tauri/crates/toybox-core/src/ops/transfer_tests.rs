//! Transfers between temporary libraries, built through the real import path.

use super::*;
use crate::config;
use crate::domain::catalog_model::{AssetContent, Catalog};
use crate::infra::registry::LibraryEntry;
use std::path::PathBuf;

fn base(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("toybox-transfer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    base
}

fn library(base: &Path, id: &str) -> LibraryHandle {
    let root = base.join(format!("root-{id}"));
    crate::infra::library::ensure_library_folder(&root).unwrap();
    let root = root.canonicalize().unwrap();
    LibraryHandle::local(&base.join("appdata"), &LibraryEntry::local(id, id, &root.to_string_lossy()))
}

fn put(library: &LibraryHandle, rel: &str, bytes: &[u8]) {
    fsio::write_bytes(&library.root.join(rel), bytes).unwrap();
}

fn put_scene(library: &LibraryHandle, rel: &str, bin: &str, images: &[&str]) {
    let images: Vec<Value> = images.iter().map(|uri| json!({"uri": uri})).collect();
    put(library, rel, &serde_json::to_vec(&json!({"asset":{"version":"2.0"},"buffers":[{"uri":bin,"byteLength":4}],"images":images})).unwrap());
}

fn wav() -> Vec<u8> {
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
    bytes.extend(vec![0u8; data as usize]);
    bytes
}

fn manifest(library: &LibraryHandle) -> Value {
    crate::infra::store::read_manifest(&library.root).unwrap()
}

fn entry(library: &LibraryHandle, id: &str) -> Option<Value> {
    manifest(library)["assets"].as_array().unwrap().iter().find(|entry| entry["id"] == id).cloned()
}

fn catalog(library: &LibraryHandle) -> Catalog {
    load_catalog_inner(library).unwrap().expect("catalog cache")
}

fn ids(library: &LibraryHandle) -> Vec<String> {
    let mut ids: Vec<String> = manifest(library)["assets"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_string()).collect();
    ids.sort();
    ids
}

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

const SKIP_COPY: TransferOptions = TransferOptions { mode: TransferMode::Copy, on_conflict: OnConflict::Skip, include_clips: false };
const NO_PROGRESS: &dyn Fn(TransferProgress) = &|_| {};

fn copy(source: &LibraryHandle, target: &LibraryHandle, wanted: &[&str], options: TransferOptions) -> TransferReport {
    transfer_between(source, target, &strings(wanted), options, NO_PROGRESS).unwrap()
}

fn item<'a>(report: &'a TransferReport, id: &str) -> &'a TransferItem {
    report.items.iter().find(|item| item.id == id).unwrap_or_else(|| panic!("no report line for {id}"))
}

fn dir_of(rel: &str) -> String {
    rel.rsplit_once('/').unwrap().0.to_string()
}

/// A source library with a model that has a variant and kept original files, a
/// second model sharing its texture, a material with a variant and a kept
/// archive, a sound and a plugin-defined skybox. All of it goes through
/// `merge_entries`, so it is stored exactly as an import stores it.
fn source_library(base: &Path) -> LibraryHandle {
    let source = library(base, "src");
    for (stem, original) in [("Crate", "full original"), ("Crate_LOD1", "low original")] {
        put_scene(&source, &format!("library/_import_s/props/{stem}.gltf"), &format!("{stem}.bin"), &["wood.png"]);
        put(&source, &format!("library/_import_s/props/{stem}.bin"), stem.as_bytes());
        put(&source, &format!("library/_import_s/props/{stem}.obj"), original.as_bytes());
    }
    put_scene(&source, "library/_import_s/props/Barrel.gltf", "Barrel.bin", &["wood.png"]);
    put(&source, "library/_import_s/props/Barrel.bin", b"barrel");
    put(&source, "library/_import_s/props/wood.png", b"wood");
    put(&source, "library/_import_s/props/shared.mtl", b"materials");
    for name in ["Stone", "Stone_1K"] {
        put(&source, &format!("library/_import_s/materials/{name}/color.png"), name.as_bytes());
    }
    put(&source, "library/_import_s/materials/Stone/stone.src", b"archive");
    put(&source, "library/_import_s/sounds/click.wav", &wav());
    put(&source, "library/_import_s/skybox/Night/px.png", b"px");
    put(&source, "library/_import_s/skybox/Night/nx.png", b"nx");
    let origin = |stem: &str| json!({"plugin":"com.example.obj","files":[format!("{stem}.obj"),"shared.mtl"],"meta":{"format":"obj","source":format!("models/{stem}.obj")}});
    let entries = vec![
        json!({"id":"uid://crate","pack":"_import_s","category":"props","file":"Crate.gltf","format":"obj","origin":origin("Crate"),
            "provenance":{"source":"Crate Pack","license":"CC0"},"tags":["Wooden Crate","prop"],
            "variants":[{"id":"uid://crate_lod","pack":"_import_s","category":"props","file":"Crate_LOD1.gltf","format":"obj","origin":origin("Crate_LOD1"),"tags":["low detail"]}]}),
        json!({"id":"uid://barrel","pack":"_import_s","category":"props","file":"Barrel.gltf"}),
        json!({"id":"uid://stone","kind":"material","pack":"_import_s","category":"stone","name":"Stone","maps":[{"role":"albedo","file":"color.png"}],
            "origin":{"plugin":"com.example.sbsar","files":["stone.src"],"meta":{"preset":"Green"}},
            "variants":[{"id":"uid://stone_small","kind":"material","pack":"_import_s","category":"stone","name":"Stone_1K","maps":[{"role":"albedo","file":"color.png"}]}]}),
        json!({"id":"uid://click","kind":"audio","pack":"_import_s","category":"sounds","file":"click.wav","tags":["click","ui"],"provenance":{"publisher":"Sound Co","url":"https://example.test/clicks"}}),
        json!({"id":"uid://night","kind":"plugin","type":"skybox","pack":"_import_s","category":"night","name":"Night","files":["px.png","nx.png"],"meta":{"layout":"cubemap"}}),
    ];
    merge_entries(&source, entries).unwrap();
    // Give the models a real pack, as an older library would have.
    let mut doc = manifest(&source);
    for entry in doc["assets"].as_array_mut().unwrap() {
        if entry["id"] == "uid://crate" {
            entry["pack"] = json!("synty_city");
            entry["variants"][0]["pack"] = json!("synty_city");
        }
    }
    crate::infra::store::write_manifest(&source.root, &doc).unwrap();
    source
}

fn open_readonly(path: &Path) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(path, permissions).unwrap();
}

fn close_readonly(path: &Path) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(path, permissions).unwrap();
}

const EVERYTHING: [&str; 5] = ["uid://crate", "uid://barrel", "uid://stone", "uid://click", "uid://night"];

#[test]
fn every_asset_kind_arrives_with_its_files_variants_and_manifest_fields() {
    let base = base("kinds");
    let source = source_library(&base);
    let target = library(&base, "dst");
    let before = manifest(&source);
    let progress = std::sync::Mutex::new(Vec::new());
    let report = transfer_between(&source, &target, &strings(&EVERYTHING), SKIP_COPY, &|p| progress.lock().unwrap().push(p)).unwrap();

    assert_eq!(report.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), EVERYTHING, "reported in the order asked");
    assert!(report.items.iter().all(|item| item.outcome == Outcome::Copied && item.reason.is_none() && !item.source_removed), "{report:?}");
    let progress = progress.lock().unwrap();
    assert_eq!(progress.last().copied(), Some(TransferProgress { done: 5, total: 5 }));

    // The source is untouched by a copy.
    assert_eq!(manifest(&source), before);
    assert_eq!(ids(&source), ids(&target));

    // The model: manifest fields survive, pack and category come back, files sit in the target's own set folders.
    let crate_entry = entry(&target, "uid://crate").unwrap();
    assert_eq!(crate_entry["format"], "obj");
    assert_eq!(crate_entry["provenance"]["source"], "Crate Pack");
    assert_eq!(crate_entry["origin"]["plugin"], "com.example.obj");
    let kept: Vec<&str> = crate_entry["origin"]["files"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(kept.len() == 2 && kept.iter().all(|path| path.starts_with("library/models/_sources/")), "{kept:?}");
    assert!(kept[0].ends_with("/Crate.obj") && kept[1].ends_with("/shared.mtl"), "{kept:?}");
    assert_eq!(crate_entry["origin"]["meta"]["source"], "models/Crate.obj");
    assert_eq!((crate_entry["pack"].as_str(), crate_entry["category"].as_str()), (Some("synty_city"), Some("props")));
    assert_eq!(crate_entry["variants"][0]["id"], "uid://crate_lod");
    assert_eq!(crate_entry["variants"][0]["pack"], "synty_city");
    assert!(!manifest(&target).to_string().contains("transferOf"), "the staging marker never reaches a manifest");
    let stored = crate_entry["storagePath"].as_str().unwrap();
    let variant_stored = crate_entry["variants"][0]["storagePath"].as_str().unwrap();
    assert!(stored.starts_with("library/models/") && variant_stored.starts_with("library/models/") && stored != variant_stored);
    assert_eq!(std::fs::read(target.root.join(kept[0])).unwrap(), b"full original");
    assert_eq!(std::fs::read(target.root.join(kept[1])).unwrap(), b"materials");
    let low: Vec<&str> = crate_entry["variants"][0]["origin"]["files"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert_eq!(std::fs::read(target.root.join(low[0])).unwrap(), b"low original");
    assert_eq!(low[1], kept[1], "the companion both models keep is stored once");
    // One texture shared by three glTFs is stored once.
    let shared: Vec<_> = std::fs::read_dir(target.root.join("library/models/_resources")).unwrap().flatten().collect();
    assert_eq!(shared.len(), 1, "the shared texture is one file");
    // The stored glTF points at it and the bytes are the source's.
    let scene: Value = serde_json::from_slice(&std::fs::read(target.root.join(stored)).unwrap()).unwrap();
    let uri = scene["images"][0]["uri"].as_str().unwrap();
    assert_eq!(std::fs::read(target.root.join(dir_of(stored)).join(uri)).unwrap(), b"wood");

    // Material, sound and plugin type.
    let stone = entry(&target, "uid://stone").unwrap();
    let archive = stone["origin"]["files"][0].as_str().unwrap().to_string();
    assert!(archive.starts_with("library/materials/_sources/") && archive.ends_with("/stone.src"), "{archive}");
    assert_eq!(stone["origin"]["meta"]["preset"], "Green");
    assert_eq!(stone["variants"][0]["id"], "uid://stone_small");
    let folder = stone["storagePath"].as_str().unwrap();
    assert!(folder.starts_with("library/materials/"));
    assert_eq!(std::fs::read(target.root.join(folder).join("color.png")).unwrap(), b"Stone");
    assert_eq!(std::fs::read(target.root.join(&archive)).unwrap(), b"archive");
    assert!(!target.root.join(folder).join("stone.src").exists());
    let click = entry(&target, "uid://click").unwrap();
    assert!(click["storagePath"].as_str().unwrap().starts_with("library/audio/"));
    let night = entry(&target, "uid://night").unwrap();
    assert_eq!(night["meta"]["layout"], "cubemap");
    assert!(night["storagePath"].as_str().unwrap().starts_with("library/skybox/"));
    assert_eq!(std::fs::read(target.root.join(night["storagePath"].as_str().unwrap()).join("nx.png")).unwrap(), b"nx");

    // The target's catalog lists everything, variants included, and no staging is left behind.
    let loaded = catalog(&target);
    assert_eq!(loaded.assets.len(), 5);
    assert_eq!(loaded.assets.iter().find(|a| a.id == "uid://crate").unwrap().variants[0].id, "uid://crate_lod");
    assert!(loaded.assets.iter().any(|a| matches!(&a.content, AssetContent::Plugin { type_id, .. } if type_id == "skybox")));
    assert!(std::fs::read_dir(target.root.join("library")).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with("_import_")));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn library_tags_and_provenance_arrive_with_a_copy_and_stay_library_data() {
    let base = base("libtags");
    let source = source_library(&base);
    let target = library(&base, "dst");
    copy(&source, &target, &["uid://crate", "uid://click"], SKIP_COPY);
    let landed = entry(&target, "uid://crate").unwrap();
    assert_eq!(landed["tags"], json!(["wooden crate", "prop"]));
    assert_eq!(landed["variants"][0]["tags"], json!(["low detail"]));
    assert_eq!(landed["provenance"]["source"], "Crate Pack");
    assert_eq!(entry(&target, "uid://click").unwrap()["tags"], json!(["click", "ui"]));
    assert_eq!(entry(&target, "uid://click").unwrap()["provenance"]["url"], "https://example.test/clicks");
    let cached = catalog(&target);
    let item = cached.assets.iter().find(|a| a.id == "uid://crate").unwrap();
    assert_eq!(item.tags, vec!["wooden crate", "prop"]);
    assert_eq!(item.variants[0].tags, vec!["low detail"]);
    assert!(!item.user.tags.iter().any(|t| t == "prop"), "they are library tags, not personal ones");
    assert!(item.user.tags.contains(&"source: Crate Pack".to_string()));
    // Editing tags in the source after the fact does not move the copy's files, and a
    // second copy of the edited item replaces it in the same folder.
    let stored = landed["storagePath"].as_str().unwrap().to_string();
    crate::ops::metadata::update_entries(&source, &[crate::ops::metadata::MetadataEdit { id: "uid://crate".into(), tags: Some(vec!["Changed".into()]), provenance: None }]).unwrap();
    let report = copy(&source, &target, &["uid://crate"], TransferOptions { on_conflict: OnConflict::Replace, ..SKIP_COPY });
    assert_eq!(item_outcome(&report, "uid://crate"), Outcome::Replaced);
    let replaced = entry(&target, "uid://crate").unwrap();
    assert_eq!(replaced["tags"], json!(["changed"]));
    assert_eq!(replaced["storagePath"].as_str().unwrap(), stored, "other tags, same folder");
    let _ = std::fs::remove_dir_all(base);
}

fn item_outcome(report: &TransferReport, id: &str) -> Outcome {
    item(report, id).outcome
}

#[test]
fn a_package_describes_the_entry_the_files_and_travels_as_plain_data() {
    let base = base("package");
    let source = source_library(&base);
    let package = export_package(&source, &strings(&["uid://crate", "uid://missing", "uid://stone"]), ExportOptions::default()).unwrap();
    assert_eq!(package.version, PACKAGE_VERSION);
    assert_eq!(package.source_library, "src");
    assert_eq!(package.assets.len(), 2);
    assert_eq!(package.failed.len(), 1);
    assert_eq!(package.failed[0].id, "uid://missing");

    let model = &package.assets[0];
    assert_eq!(model.entry, entry(&source, "uid://crate").unwrap(), "the entry exactly as the manifest records it");
    assert_eq!(model.kind, "model");
    let names: Vec<&str> = model.files.iter().map(|f| basename(&f.path)).collect();
    for expected in ["Crate.gltf", "Crate.bin", "Crate.obj", "shared.mtl", "Crate_LOD1.gltf", "Crate_LOD1.bin", "Crate_LOD1.obj"] {
        assert!(names.contains(&expected), "{expected} in {names:?}");
    }
    assert_eq!(model.files.iter().filter(|f| f.path.contains("_resources")).count(), 1, "a texture is listed once");
    for file in &model.files {
        let bytes = std::fs::read(source.root.join(&file.path)).unwrap();
        assert_eq!((file.size, file.sha256.as_str()), (bytes.len() as u64, sha_hex(&bytes).as_str()));
        assert!(file.path.starts_with("library/"));
    }
    let material = &package.assets[1];
    assert_eq!(material.kind, "material");
    assert!(material.files.iter().any(|f| f.path.ends_with("stone.src")) && material.files.iter().any(|f| f.path.ends_with("Stone_1K/color.png") || f.path.contains("color.png")));

    // It is plain data: it survives JSON, and ingest works from the copy.
    let wire = serde_json::to_string(&package).unwrap();
    let back: TransferPackage = serde_json::from_str(&wire).unwrap();
    assert_eq!(back, package);
    let target = library(&base, "dst");
    let reader = local_reader(&source);
    let report = ingest_package(&target, &back, IngestOptions::default(), &reader, NO_PROGRESS).unwrap();
    assert_eq!(report.count(Outcome::Copied), 2);
    assert_eq!(report.count(Outcome::Failed), 1);
    assert_eq!(item(&report, "uid://missing").reason.as_deref(), Some("No longer in the source library"));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn existing_items_are_skipped_unless_replacing_was_asked_for() {
    let base = base("conflict");
    let source = source_library(&base);
    let target = library(&base, "dst");
    let wanted = ["uid://crate", "uid://barrel", "uid://stone"];
    copy(&source, &target, &wanted, SKIP_COPY);
    // The target's own work on its copy.
    let mut own = catalog(&target);
    own.assets.iter_mut().find(|a| a.id == "uid://crate").unwrap().user.favorite = true;
    own.assets.iter_mut().find(|a| a.id == "uid://crate").unwrap().user.tags = vec!["mine".into()];
    save_catalog_inner(&target, &own).unwrap();
    let kept = manifest(&target);
    let old_folder = entry(&target, "uid://stone").unwrap()["storagePath"].as_str().unwrap().to_string();
    let old_barrel = entry(&target, "uid://barrel").unwrap()["storagePath"].clone();

    // The source changes two of them.
    let crate_dir = dir_of(entry(&source, "uid://crate").unwrap()["storagePath"].as_str().unwrap());
    std::fs::write(source.root.join(&crate_dir).join("Crate.bin"), b"changed").unwrap();
    let stone_dir = entry(&source, "uid://stone").unwrap()["storagePath"].as_str().unwrap().to_string();
    std::fs::write(source.root.join(&stone_dir).join("color.png"), b"redder").unwrap();

    let skipped = copy(&source, &target, &wanted, SKIP_COPY);
    assert!(skipped.items.iter().all(|i| i.outcome == Outcome::Skipped && i.reason.is_some()), "{skipped:?}");
    assert_eq!(manifest(&target), kept, "skipping writes nothing");

    let replaced = copy(&source, &target, &wanted, TransferOptions { on_conflict: OnConflict::Replace, ..SKIP_COPY });
    assert!(replaced.items.iter().all(|i| i.outcome == Outcome::Replaced), "{replaced:?}");
    let new_folder = entry(&target, "uid://stone").unwrap()["storagePath"].as_str().unwrap().to_string();
    assert_ne!(new_folder, old_folder);
    assert_eq!(std::fs::read(target.root.join(&new_folder).join("color.png")).unwrap(), b"redder");
    assert!(!target.root.join(&old_folder).exists(), "the replaced material's old folder is retired");
    let new_crate = entry(&target, "uid://crate").unwrap();
    assert_eq!(std::fs::read(target.root.join(dir_of(new_crate["storagePath"].as_str().unwrap())).join("Crate.bin")).unwrap(), b"changed");
    assert_eq!(entry(&target, "uid://barrel").unwrap()["storagePath"], old_barrel, "an unchanged item lands on the same files");
    assert_eq!(ids(&target), ids_of(&wanted_with_variants()), "no duplicate registrations");
    // The target's favourites and tags on a replaced item stay.
    let crate_after = catalog(&target).assets.into_iter().find(|a| a.id == "uid://crate").unwrap();
    assert!(crate_after.user.favorite && crate_after.user.tags.contains(&"mine".to_string()));
    let _ = std::fs::remove_dir_all(base);
}

fn wanted_with_variants() -> Vec<&'static str> {
    vec!["uid://barrel", "uid://crate", "uid://stone"]
}

fn ids_of(ids: &[&str]) -> Vec<String> {
    strings(ids)
}

#[test]
fn a_move_deletes_only_what_was_copied() {
    let base = base("move");
    let source = source_library(&base);
    let target = library(&base, "dst");
    // Barrel is already in the target (Skip leaves it, so it must stay in the source).
    copy(&source, &target, &["uid://barrel"], SKIP_COPY);
    // The material's map vanishes from the source: it cannot be described, so it must stay.
    let stone_dir = entry(&source, "uid://stone").unwrap()["storagePath"].as_str().unwrap().to_string();
    std::fs::remove_file(source.root.join(&stone_dir).join("color.png")).unwrap();
    let crate_files: Vec<String> = {
        let dir = dir_of(entry(&source, "uid://crate").unwrap()["storagePath"].as_str().unwrap());
        std::fs::read_dir(source.root.join(&dir)).unwrap().flatten().map(|e| e.path().to_string_lossy().to_string()).collect()
    };

    let report = copy(&source, &target, &["uid://crate", "uid://barrel", "uid://stone", "uid://click"],
        TransferOptions { mode: TransferMode::Move, ..SKIP_COPY });
    assert_eq!(item(&report, "uid://crate").outcome, Outcome::Copied);
    assert!(item(&report, "uid://crate").source_removed, "{report:?}");
    assert_eq!(item(&report, "uid://click").outcome, Outcome::Copied);
    assert!(item(&report, "uid://click").source_removed);
    assert_eq!(item(&report, "uid://barrel").outcome, Outcome::Skipped);
    assert!(!item(&report, "uid://barrel").source_removed);
    assert_eq!(item(&report, "uid://stone").outcome, Outcome::Failed);
    assert!(!item(&report, "uid://stone").source_removed);

    assert_eq!(ids(&source), strings(&["uid://barrel", "uid://night", "uid://stone"]));
    assert_eq!(ids(&target), strings(&["uid://barrel", "uid://click", "uid://crate"]));
    assert!(crate_files.iter().all(|f| !Path::new(f).exists() || f.contains("_resources")), "the moved model's own files left the source");
    // The texture barrel still needs is still in the source.
    let barrel = entry(&source, "uid://barrel").unwrap();
    assert!(source.root.join(barrel["storagePath"].as_str().unwrap()).exists());
    assert!(catalog(&source).assets.iter().all(|a| a.id != "uid://crate"));
    // And what arrived is complete.
    let moved = entry(&target, "uid://crate").unwrap();
    assert!(target.root.join(moved["storagePath"].as_str().unwrap()).exists());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_failing_asset_does_not_stop_the_others_and_is_never_removed_by_a_move() {
    let base = base("failing");
    let source = source_library(&base);
    let target = library(&base, "dst");
    let package = export_package(&source, &strings(&EVERYTHING), ExportOptions::default()).unwrap();
    // The sound's bytes change after the package was described.
    let sound = package.assets.iter().find(|a| a.id == "uid://click").unwrap().files[0].path.clone();
    let honest = local_reader(&source);
    let reader = |rel: &str| -> AppResult<Box<dyn Read>> {
        if rel == sound { Ok(Box::new(std::io::Cursor::new(b"tampered".to_vec()))) } else { honest(rel) }
    };
    let report = ingest_package(&target, &package, IngestOptions::default(), &reader, NO_PROGRESS).unwrap();
    assert_eq!(item(&report, "uid://click").outcome, Outcome::Failed);
    assert!(item(&report, "uid://click").reason.as_deref().unwrap().contains("changed while"));
    assert_eq!(report.count(Outcome::Copied), 4);
    assert_eq!(ids(&target), strings(&["uid://barrel", "uid://crate", "uid://night", "uid://stone"]));
    assert!(target.root.read_dir().unwrap().flatten().count() > 0);
    assert!(entry(&target, "uid://click").is_none());

    // A package that omits a file one item needs fails that item only.
    let target2 = library(&base, "dst2");
    let mut partial = package.clone();
    partial.assets.iter_mut().find(|a| a.id == "uid://night").unwrap().files.pop();
    let report = ingest_package(&target2, &partial, IngestOptions::default(), &local_reader(&source), NO_PROGRESS).unwrap();
    assert_eq!(item(&report, "uid://night").outcome, Outcome::Failed);
    assert!(item(&report, "uid://night").reason.as_deref().unwrap().contains("does not list"));
    assert_eq!(report.count(Outcome::Copied), 4);

    // The reader never leaves the library.
    let reader = local_reader(&source);
    assert!(reader("library/../_library_config/catalog.json").is_err());
    assert!(reader("_library_config/catalog.json").is_err());
    assert!(reader("thumbs/../x.png").is_err());
    assert!(reader("library/nothing/here.png").is_err());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn favourites_tags_collections_provenance_and_thumbnails_follow_a_new_item() {
    let base = base("metadata");
    let source = source_library(&base);
    let target = library(&base, "dst");
    let mut cached = catalog(&source);
    for asset in &mut cached.assets {
        match asset.id.as_str() {
            "uid://crate" => {
                asset.user.favorite = true;
                asset.user.tags = vec!["wooden".into(), "prop".into()];
                asset.user.collections = vec!["Kitchen".into()];
                asset.user.preview_asset_id = Some("uid://barrel".into());
                asset.user.provenance = Some(serde_json::from_value(json!({"filesystemPath":"P:/packs/crate"})).unwrap());
                asset.variants[0].user.favorite = true;
                asset.variants[0].user.preview_asset_id = Some("uid://elsewhere".into());
            }
            "uid://barrel" => asset.user.tags = vec!["round".into()],
            _ => {}
        }
    }
    save_catalog_inner(&source, &cached).unwrap();
    fsio::write_bytes(&source.thumb_path("uid://crate"), b"png-crate").unwrap();
    fsio::write_bytes(&source.thumb_path("uid://crate_lod"), b"png-lod").unwrap();

    copy(&source, &target, &["uid://crate"], SKIP_COPY);
    let landed = catalog(&target).assets.into_iter().find(|a| a.id == "uid://crate").unwrap();
    assert!(landed.user.favorite);
    for tag in ["wooden", "prop"] {
        assert!(landed.user.tags.contains(&tag.to_string()), "{tag} in {:?}", landed.user.tags);
    }
    assert!(landed.user.tags.iter().any(|t| t.to_lowercase().contains("crate pack")) || landed.user.provenance.is_some());
    assert_eq!(landed.user.collections, vec!["Kitchen".to_string()]);
    assert_eq!(serde_json::to_value(&landed.user.provenance).unwrap()["filesystemPath"], "P:/packs/crate");
    assert_eq!(landed.user.preview_asset_id, None, "a preview choice naming an item the target lacks is dropped");
    assert!(landed.variants[0].user.favorite && landed.variants[0].user.preview_asset_id.is_none());
    assert_eq!(std::fs::read(target.thumb_path("uid://crate")).unwrap(), b"png-crate");
    assert_eq!(std::fs::read(target.thumb_path("uid://crate_lod")).unwrap(), b"png-lod");
    assert!(matches!(catalog(&target).assets[0].thumb.state, crate::domain::catalog_model::ThumbState::Ready));

    // The provenance in the manifest came with the entry.
    assert_eq!(entry(&target, "uid://crate").unwrap()["provenance"]["license"], "CC0");

    // With the barrel arriving too, the choice stays valid.
    let target2 = library(&base, "dst2");
    copy(&source, &target2, &["uid://crate", "uid://barrel"], SKIP_COPY);
    let both = catalog(&target2);
    assert_eq!(both.assets.iter().find(|a| a.id == "uid://crate").unwrap().user.preview_asset_id.as_deref(), Some("uid://barrel"));
    assert!(both.assets.iter().find(|a| a.id == "uid://barrel").unwrap().user.tags.contains(&"round".to_string()));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn the_same_raw_id_in_both_libraries_is_a_conflict_that_never_touches_the_other_side() {
    let base = base("same-id");
    let source = source_library(&base);
    let target = library(&base, "dst");
    // The target has a different thing under the same id, with its own metadata.
    put(&target, "library/_import_t/materials/Mine/color.png", b"mine");
    merge_entries(&target, vec![json!({"id":"uid://crate","kind":"material","pack":"_import_t","category":"stone","name":"Mine","maps":[{"role":"albedo","file":"color.png"}]})]).unwrap();
    let mut own = catalog(&target);
    own.assets[0].user.favorite = true;
    save_catalog_inner(&target, &own).unwrap();
    let theirs = entry(&target, "uid://crate").unwrap();

    // Skip: nothing changes anywhere, and a move leaves the source item alone.
    let moved = copy(&source, &target, &["uid://crate"], TransferOptions { mode: TransferMode::Move, ..SKIP_COPY });
    assert_eq!(item(&moved, "uid://crate").outcome, Outcome::Skipped);
    assert_eq!(entry(&target, "uid://crate").unwrap(), theirs);
    assert!(entry(&source, "uid://crate").is_some());

    // Replace by copy: the target now holds the source's model, the source is unchanged,
    // and the target's own favourite on that id is still there.
    let source_entry = entry(&source, "uid://crate").unwrap();
    let report = copy(&source, &target, &["uid://crate"], TransferOptions { on_conflict: OnConflict::Replace, ..SKIP_COPY });
    assert_eq!(item(&report, "uid://crate").outcome, Outcome::Replaced);
    assert_eq!(entry(&source, "uid://crate").unwrap(), source_entry);
    let replaced = entry(&target, "uid://crate").unwrap();
    assert!(replaced["storagePath"].as_str().unwrap().starts_with("library/models/"));
    assert!(replaced.get("kind").is_none_or(|kind| kind != "material"));
    assert_eq!(ids(&target), strings(&["uid://crate"]));
    assert!(catalog(&target).assets[0].user.favorite);

    // A variant id that already belongs to another item of the target is refused for that item only.
    let target2 = library(&base, "dst2");
    put(&target2, "library/_import_t/props/Lod.gltf", &serde_json::to_vec(&json!({"asset":{"version":"2.0"}})).unwrap());
    merge_entries(&target2, vec![json!({"id":"uid://crate_lod","pack":"_import_t","category":"props","file":"Lod.gltf"})]).unwrap();
    let report = copy(&source, &target2, &["uid://crate", "uid://barrel"], SKIP_COPY);
    assert_eq!(item(&report, "uid://crate").outcome, Outcome::Failed);
    assert!(item(&report, "uid://crate").reason.as_deref().unwrap().contains("uid://crate_lod"));
    assert_eq!(item(&report, "uid://barrel").outcome, Outcome::Copied);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn extracted_clips_stay_behind_unless_asked_for_and_keep_naming_their_source() {
    let base = base("clips");
    let source = library(&base, "src");
    let doc = json!({"asset":{"version":"2.0"},"nodes":[{"name":"Root","children":[1]},{"name":"Hip","translation":[0,1,0]},{"name":"Body","mesh":0,"skin":0}],
        "skins":[{"joints":[0,1],"inverseBindMatrices":2}],"meshes":[{}],"images":[{"uri":"atlas.png"}],
        "buffers":[{"uri":"Hero.bin","byteLength":128}],
        "bufferViews":[{"buffer":0,"byteOffset":64,"byteLength":8},{"buffer":0,"byteOffset":72,"byteLength":24}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[1]},{"bufferView":1,"componentType":5126,"count":2,"type":"VEC3"}],
        "animations":[{"name":"Walk","samplers":[{"input":0,"output":1}],"channels":[{"sampler":0,"target":{"node":1,"path":"translation"}}]}]});
    put(&source, "library/_import_h/chars/Hero.gltf", &serde_json::to_vec(&doc).unwrap());
    put(&source, "library/_import_h/chars/Hero.bin", &(0..128u8).collect::<Vec<_>>());
    put(&source, "library/_import_h/chars/atlas.png", b"atlas");
    merge_entries(&source, vec![json!({"id":"uid://hero","pack":"_import_h","category":"chars","file":"Hero.gltf"})]).unwrap();
    let clip_id = ids(&source).into_iter().find(|id| id != "uid://hero").expect("an extracted clip");
    assert_eq!(entry(&source, "uid://hero").unwrap()["animationsExtracted"], true);

    let alone = library(&base, "alone");
    let report = copy(&source, &alone, &["uid://hero"], SKIP_COPY);
    assert_eq!(report.items.len(), 1);
    assert_eq!(ids(&alone), strings(&["uid://hero"]), "the clips are not dragged along");
    assert_eq!(entry(&alone, "uid://hero").unwrap()["animationsExtracted"], true, "and the model is not re-extracted");

    let with = library(&base, "with");
    let report = copy(&source, &with, &["uid://hero"], TransferOptions { include_clips: true, ..SKIP_COPY });
    assert_eq!(report.items.len(), 2, "{report:?}");
    assert!(report.items.iter().all(|i| i.outcome == Outcome::Copied));
    assert_eq!(ids(&with), ids(&source));
    let clip = catalog(&with).assets.into_iter().find(|a| a.id == clip_id).unwrap();
    let AssetContent::Animation { clip: meta, fileset } = &clip.content else { panic!("an animation") };
    assert_eq!(meta.source_id, "uid://hero", "the clip still names its source model by id");
    assert!(with.root.join(&fileset.gltf).exists());

    // A clip on its own refers to a model the target lacks; it arrives all the same.
    let only_clip = library(&base, "only-clip");
    let report = copy(&source, &only_clip, &[clip_id.as_str()], SKIP_COPY);
    assert_eq!(item(&report, &clip_id).outcome, Outcome::Copied);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn libraries_that_cannot_be_written_are_refused_and_a_library_is_not_its_own_target() {
    let base = base("readonly");
    let source = source_library(&base);
    let target = library(&base, "dst");
    assert!(library_rules::is_writable(&target.root));
    let manifest_path = target.root.join(config::LIBRARY_DB_REL);
    open_readonly(&manifest_path);
    let writable = library_rules::is_writable(&target.root);
    let attempt = transfer_between(&source, &target, &strings(&["uid://barrel"]), SKIP_COPY, NO_PROGRESS);
    close_readonly(&manifest_path);
    assert!(!writable, "a read-only library database means a read-only library");
    assert!(attempt.unwrap_err().to_string().contains("read-only"));
    assert!(entry(&target, "uid://barrel").is_none());

    open_readonly(&source.root.join(config::LIBRARY_DB_REL));
    let attempt = transfer_between(&source, &target, &strings(&["uid://barrel"]), TransferOptions { mode: TransferMode::Move, ..SKIP_COPY }, NO_PROGRESS);
    // Copying out of a read-only library is fine.
    let copied = transfer_between(&source, &target, &strings(&["uid://barrel"]), SKIP_COPY, NO_PROGRESS);
    close_readonly(&source.root.join(config::LIBRARY_DB_REL));
    assert!(attempt.unwrap_err().to_string().contains("read-only"));
    assert_eq!(copied.unwrap().count(Outcome::Copied), 1);

    assert!(transfer_between(&source, &source, &strings(&["uid://barrel"]), SKIP_COPY, NO_PROGRESS).is_err());
    assert!(!library_rules::is_writable(&base.join("not-a-library")));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn an_item_the_target_rejects_at_commit_does_not_sink_the_items_committed_with_it() {
    let base = base("commit-failure");
    let source = source_library(&base);
    let target = library(&base, "dst");
    // Described and copied faithfully, but not a sound the target can index.
    let click = entry(&source, "uid://click").unwrap()["storagePath"].as_str().unwrap().to_string();
    std::fs::write(source.root.join(&click), b"not audio at all").unwrap();
    let report = copy(&source, &target, &EVERYTHING, SKIP_COPY);
    assert_eq!(item(&report, "uid://click").outcome, Outcome::Failed);
    assert!(item(&report, "uid://click").reason.is_some());
    assert_eq!(report.count(Outcome::Copied), 4, "{report:?}");
    assert_eq!(ids(&target), strings(&["uid://barrel", "uid://crate", "uid://night", "uid://stone"]));
    assert_eq!(catalog(&target).assets.len(), 4);
    assert!(std::fs::read_dir(target.root.join("library")).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with("_import_")));

    // A move of the same set removes the four that arrived and keeps the sound.
    let target2 = library(&base, "dst2");
    let report = copy(&source, &target2, &EVERYTHING, TransferOptions { mode: TransferMode::Move, ..SKIP_COPY });
    assert_eq!(ids(&source), strings(&["uid://click"]));
    assert!(report.items.iter().filter(|i| i.outcome == Outcome::Copied).all(|i| i.source_removed));
    assert!(!item(&report, "uid://click").source_removed);
    let _ = std::fs::remove_dir_all(base);
}

// --- Models whose glTF carries an embedded (`data:`) image ---------------------------
//
// FBX2glTF writes an image that has no file as a `data:image/png;base64,...` uri.
// That is not a file: it must never be listed, hashed, copied, owned or deleted.

const PIXEL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7w==";
const OTHER_PIXEL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

fn embedded_base(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    base(&format!("embedded-{name}-{}", COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)))
}

/// A library with the model `uid://crate_mesh` (one external image, one embedded
/// image) and an unrelated sound, imported the normal way.
fn embedded_library(base: &Path, id: &str) -> LibraryHandle {
    let library = library(base, id);
    put_scene(&library, "library/_import_e/props/crate_mesh.gltf", "crate_mesh.bin", &["wood.png", PIXEL]);
    put(&library, "library/_import_e/props/crate_mesh.bin", b"crate");
    put(&library, "library/_import_e/props/wood.png", b"wood");
    put(&library, "library/_import_e/sounds/click.wav", &wav());
    merge_entries(&library, vec![
        json!({"id":"uid://crate_mesh","pack":"_import_e","category":"props","file":"crate_mesh.gltf"}),
        json!({"id":"uid://click","kind":"audio","pack":"_import_e","category":"sounds","file":"click.wav"}),
    ]).unwrap();
    library
}

/// What an older version wrote into the cache file: the embedded image as a texture
/// path. The file is where the old cache was; the library's databases only ever
/// receive such a record through the migration, which reads it like any other reader.
fn plant_old_texture_record(library: &LibraryHandle) -> Value {
    let mut doc = serde_json::to_value(catalog(library)).unwrap();
    for asset in doc["assets"].as_array_mut().unwrap() {
        if asset["id"] == "uid://crate_mesh" {
            let dir = dir_of(asset["fileset"]["gltf"].as_str().unwrap());
            asset["fileset"]["textures"].as_array_mut().unwrap().push(json!(format!("{dir}/{PIXEL}")));
        }
    }
    assert!(serde_json::to_string(&doc).unwrap().contains("data:image"));
    std::fs::write(&library.cache_path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
    doc
}

fn model_files(library: &LibraryHandle) -> (PathBuf, PathBuf) {
    let stored = entry(library, "uid://crate_mesh").unwrap()["storagePath"].as_str().unwrap().to_string();
    (library.root.join(&stored), library.root.join(&stored).with_extension("bin"))
}

fn delete_ids(library: &LibraryHandle, ids: &[&str]) -> AppResult<Catalog> {
    super::super::delete::delete_from_library(&library.root, &library.cache_path, &strings(ids))?;
    Ok(load_catalog_inner(library)?.expect("catalog cache"))
}

#[test]
fn an_embedded_image_is_neither_a_texture_nor_rewritten_when_the_model_is_stored() {
    let base = embedded_base("storage");
    let library = embedded_library(&base, "one");
    let loaded = catalog(&library);
    let model = loaded.assets.iter().find(|a| a.id == "uid://crate_mesh").unwrap();
    let textures = &model.fileset().unwrap().textures;
    assert_eq!(textures.len(), 1, "{textures:?}");
    assert!(textures[0].starts_with("library/models/_resources/") && textures[0].ends_with(".png"), "{textures:?}");
    let mut files = Vec::new();
    model.collect_content_files(&mut files);
    assert!(files.iter().all(|f| !f.contains("data:")), "{files:?}");
    assert!(!serde_json::to_string(&loaded).unwrap().contains("data:image"), "the cache records no embedded image");

    let (gltf, _) = model_files(&library);
    let scene: Value = serde_json::from_slice(&std::fs::read(&gltf).unwrap()).unwrap();
    assert!(scene["images"][0]["uri"].as_str().unwrap().starts_with("../_resources/"));
    assert_eq!(scene["images"][1]["uri"], PIXEL, "the embedded image stays embedded");

    // The embedded bytes are part of the identity: other bytes, another set folder.
    put_scene(&library, "library/_import_e/props/crate_other.gltf", "crate_other.bin", &["wood.png", OTHER_PIXEL]);
    put(&library, "library/_import_e/props/crate_other.bin", b"crate");
    put(&library, "library/_import_e/props/wood.png", b"wood");
    merge_entries(&library, vec![json!({"id":"uid://crate_other","pack":"_import_e","category":"props","file":"crate_other.gltf"})]).unwrap();
    let other = entry(&library, "uid://crate_other").unwrap();
    let first = entry(&library, "uid://crate_mesh").unwrap();
    assert_ne!(dir_of(other["storagePath"].as_str().unwrap()), dir_of(first["storagePath"].as_str().unwrap()));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn old_caches_that_list_the_embedded_image_as_a_file_are_read_without_it() {
    let base = embedded_base("oldcache");
    let library = embedded_library(&base, "one");
    let planted = plant_old_texture_record(&library);
    // The migration reads the old file the way the loader always did.
    let adopted: Catalog = serde_json::from_value(crate::domain::catalog_migrate::upgrade(planted)).unwrap();
    let loaded = adopted;
    let model = loaded.assets.iter().find(|a| a.id == "uid://crate_mesh").unwrap();
    assert_eq!(model.fileset().unwrap().textures.len(), 1);
    let mut files = Vec::new();
    model.collect_content_files(&mut files);
    assert!(files.iter().all(|f| !f.contains("data:")), "{files:?}");
    // The raw record is tolerated by every reader, not only the loader.
    let raw: Catalog = serde_json::from_slice(&std::fs::read(&library.cache_path).unwrap()).unwrap();
    assert_eq!(raw.assets.iter().find(|a| a.id == "uid://crate_mesh").unwrap().fileset().unwrap().textures.len(), 1);
    // A rebuild from the manifest never writes it again.
    let rebuilt = crate::ops::scan::build_catalog_from_seed(&library.root_str()).unwrap();
    assert!(!serde_json::to_string(&rebuilt).unwrap().contains("data:image"));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn deleting_works_beside_a_model_with_an_embedded_image_and_removes_only_real_files() {
    for old_cache in [false, true] {
        let base = embedded_base("delete");
        let library = embedded_library(&base, "one");
        if old_cache {
            plant_old_texture_record(&library);
        }
        let (gltf, bin) = model_files(&library);
        let click = library.root.join(entry(&library, "uid://click").unwrap()["storagePath"].as_str().unwrap());
        assert!(gltf.exists() && bin.exists() && click.exists());

        // An unrelated item can be deleted although the library holds such a model.
        let after = delete_ids(&library, &["uid://click"]).unwrap();
        assert_eq!(after.assets.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["uid://crate_mesh"]);
        assert!(!click.exists());
        assert!(gltf.exists() && bin.exists(), "the model is untouched");

        // The model itself can be deleted; its own files go, nothing else is looked for.
        let shared = library.root.join("library/models/_resources");
        assert_eq!(std::fs::read_dir(&shared).unwrap().flatten().count(), 1);
        let after = delete_ids(&library, &["uid://crate_mesh"]).unwrap();
        assert!(after.assets.is_empty());
        assert!(!gltf.exists() && !bin.exists());
        assert!(std::fs::read_dir(&shared).unwrap().flatten().next().is_none(), "its real image went with it");
        assert!(ids(&library).is_empty());
        let _ = std::fs::remove_dir_all(base);
    }
}

#[test]
fn the_other_item_survives_deleting_the_embedded_image_model() {
    let base = embedded_base("delete-model");
    let library = embedded_library(&base, "one");
    plant_old_texture_record(&library);
    let (gltf, bin) = model_files(&library);
    let click = library.root.join(entry(&library, "uid://click").unwrap()["storagePath"].as_str().unwrap());
    delete_ids(&library, &["uid://crate_mesh"]).unwrap();
    assert!(!gltf.exists() && !bin.exists());
    assert!(click.exists());
    assert_eq!(ids(&library), strings(&["uid://click"]));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_package_and_a_move_carry_only_the_real_files_of_a_model_with_an_embedded_image() {
    let base = embedded_base("transfer");
    let source = embedded_library(&base, "src");
    plant_old_texture_record(&source);
    let package = export_package(&source, &strings(&["uid://crate_mesh", "uid://click"]), ExportOptions::default()).unwrap();
    assert!(package.failed.is_empty(), "{:?}", package.failed);
    let model = package.assets.iter().find(|a| a.id == "uid://crate_mesh").unwrap();
    assert_eq!(model.files.len(), 3, "glTF, bin and the one real image: {:?}", model.files);
    assert!(model.files.iter().all(|f| !f.path.contains("data:") && source.root.join(&f.path).is_file()));

    let target = library(&base, "dst");
    let report = copy(&source, &target, &["uid://crate_mesh", "uid://click"], TransferOptions { mode: TransferMode::Move, ..SKIP_COPY });
    assert!(report.items.iter().all(|i| i.outcome == Outcome::Copied && i.source_removed), "{report:?}");
    assert!(ids(&source).is_empty());
    assert_eq!(ids(&target), strings(&["uid://click", "uid://crate_mesh"]));
    let (gltf, bin) = model_files(&target);
    assert!(gltf.exists() && bin.exists());
    let scene: Value = serde_json::from_slice(&std::fs::read(&gltf).unwrap()).unwrap();
    assert_eq!(scene["images"][1]["uri"], PIXEL);
    let loaded = catalog(&target);
    let textures = &loaded.assets.iter().find(|a| a.id == "uid://crate_mesh").unwrap().fileset().unwrap().textures;
    assert_eq!(textures.len(), 1);

    // And back again.
    let report = copy(&target, &source, &["uid://crate_mesh"], TransferOptions { mode: TransferMode::Move, ..SKIP_COPY });
    assert!(item(&report, "uid://crate_mesh").source_removed, "{report:?}");
    assert_eq!(ids(&source), strings(&["uid://crate_mesh"]));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn exports_of_a_model_with_an_embedded_image() {
    use crate::domain::export_model::{ExportCopyReq, ExportGlbReq};
    let base = embedded_base("export");
    let library = embedded_library(&base, "one");
    plant_old_texture_record(&library);
    let ids = strings(&["uid://crate_mesh"]);

    // Planned paths and fingerprints do not look for the embedded image.
    let planned = crate::ops::export_plugin::preview_export(&library, &ids, "copy", false).unwrap();
    assert!(planned.iter().all(|p| !p.contains("data:")), "{planned:?}");
    assert_eq!(planned.len(), 3, "{planned:?}");
    crate::ops::export_plugin::asset_fingerprints(&library, &ids, true).unwrap();

    // Copy: the glTF keeps the embedded image, only the real image is copied.
    let target = base.join("out-copy");
    let report = crate::ops::export_copy::export_copy(&library, &ExportCopyReq { asset_ids: ids.clone(), target_dir: target.to_string_lossy().into(), preserve_structure: false }).unwrap();
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let written: Value = serde_json::from_slice(&std::fs::read(target.join("crate_mesh.gltf")).unwrap()).unwrap();
    assert_eq!(written["images"][1]["uri"], PIXEL);
    assert!(written["images"][0]["uri"].as_str().unwrap().starts_with("./textures/"));
    assert_eq!(std::fs::read_dir(target.join("textures")).unwrap().count(), 1);

    // GLB: the embedded image is decoded into the binary chunk like the file's.
    let target = base.join("out-glb");
    let report = crate::ops::export_glb::export_glb(&library, &ExportGlbReq { asset_ids: ids.clone(), target_dir: target.to_string_lossy().into(), preserve_structure: false }).unwrap();
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let glb = std::fs::read(target.join("crate_mesh.glb")).unwrap();
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let doc: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
    for index in 0..2 {
        assert!(doc["images"][index].get("uri").is_none(), "image {index} is inside the GLB: {}", doc["images"][index]);
        assert!(doc["images"][index]["bufferView"].is_number());
        assert_eq!(doc["images"][index]["mimeType"], "image/png");
    }
    let view = &doc["bufferViews"][doc["images"][1]["bufferView"].as_u64().unwrap() as usize];
    let binary = &glb[20 + json_len + 8..];
    let offset = view["byteOffset"].as_u64().unwrap() as usize;
    assert_eq!(&binary[offset..offset + 8], b"\x89PNG\r\n\x1a\n");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn building_a_catalog_never_records_an_embedded_image_as_a_texture() {
    let base = embedded_base("build");
    let library = embedded_library(&base, "one");
    let rebuilt = crate::ops::scan::build_catalog_from_seed(&library.root_str()).unwrap();
    let model = rebuilt.assets.iter().find(|a| a.id == "uid://crate_mesh").unwrap();
    let gltf = model.fileset().unwrap().gltf.clone();
    let facets = crate::ops::catalog_build::read_asset_facets(&library.root_str(), &gltf).unwrap();
    assert_eq!(facets.textures.len(), 1, "{:?}", facets.textures);
    assert!(facets.textures.iter().all(|t| !t.contains("data:")));
    let _ = std::fs::remove_dir_all(base);
}
