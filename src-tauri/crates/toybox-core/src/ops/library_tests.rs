//! Two libraries that hold the same asset id stay independent: separate caches
//! and thumbnails, and deleting from one never touches the other.

use serde_json::{json, Value};

use super::catalog::save_catalog_inner;
use super::catalog_build::build_catalog;
use super::delete::delete_from_library;
use super::thumbs::pending_ids;
use crate::domain::catalog_model::Catalog;
use crate::infra::fsio;
use crate::infra::registry::{LibraryEntry, LibraryHandle};

struct Fixture {
    data: std::path::PathBuf,
    handle: LibraryHandle,
    manifest: Value,
}

fn library(base: &std::path::Path, id: &str) -> Fixture {
    let root = base.join(format!("root-{id}"));
    let entry = json!({"id":"uid://same","name":"same","kind":"material","pack":"p","category":"stone","maps":[{"role":"albedo","file":"tex.png"}]});
    let manifest = json!({"schemaVersion":1,"assets":[entry]});
    crate::infra::store::write_manifest(&root, &manifest).unwrap();
    fsio::write_bytes(&root.join("library/p/materials/same/tex.png"), id.as_bytes()).unwrap();
    let data = base.join("appdata");
    let handle = LibraryHandle::local(&data, &LibraryEntry::local(id, id, &root.to_string_lossy()));
    Fixture { data, handle, manifest }
}

fn catalog_of(fixture: &Fixture) -> Catalog {
    build_catalog(&fixture.handle.root_str(), crate::domain::seed::parse_seed_entries(&fixture.manifest).unwrap()).unwrap()
}

#[test]
fn equal_asset_ids_in_two_libraries_have_independent_caches_thumbnails_and_deletes() {
    let base = std::env::temp_dir().join(format!("toybox-two-libraries-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let a = library(&base, "aaa");
    let b = library(&base, "bbb");
    let (mut ca, mut cb) = (catalog_of(&a), catalog_of(&b));
    assert_eq!(ca.assets[0].id, cb.assets[0].id, "the same raw id in both libraries");
    ca.assets[0].user.favorite = true;
    cb.assets[0].user.tags = vec!["only-b".into()];
    save_catalog_inner(&a.handle, &ca).unwrap();
    save_catalog_inner(&b.handle, &cb).unwrap();

    // Separate databases under separate folders, each stamped with its own root.
    assert_ne!(a.handle.cache_path, b.handle.cache_path);
    assert_ne!(a.handle.user_db_path(), b.handle.user_db_path());
    assert_ne!(a.handle.library_db_path(), b.handle.library_db_path());
    let read = |h: &LibraryHandle| -> Catalog { crate::infra::store::Store::for_handle(h).load_catalog().unwrap().unwrap() };
    assert!(read(&a.handle).assets[0].user.favorite && read(&a.handle).assets[0].user.tags.is_empty());
    assert!(!read(&b.handle).assets[0].user.favorite && read(&b.handle).assets[0].user.tags == vec!["only-b".to_string()]);
    assert_eq!(read(&a.handle).library_root, a.handle.root_str());
    assert_eq!(read(&b.handle).library_root, b.handle.root_str());

    // A thumbnail in one library does not satisfy the other.
    fsio::write_bytes(&a.handle.thumb_path("uid://same"), b"png-a").unwrap();
    fsio::write_bytes(&b.handle.thumb_path("uid://same"), b"png-b").unwrap();
    assert!(pending_ids(&a.handle, &read(&a.handle)).is_empty());
    std::fs::remove_file(b.handle.thumb_path("uid://same")).unwrap();
    assert!(pending_ids(&a.handle, &read(&a.handle)).is_empty());
    assert_eq!(pending_ids(&b.handle, &read(&b.handle)), vec!["uid://same".to_string()]);
    fsio::write_bytes(&b.handle.thumb_path("uid://same"), b"png-b").unwrap();

    // Deleting the id from A removes A's registration, files, cache entry and thumbnail only.
    let a_files = AssetFiles::of(&ca).0;
    let b_files = AssetFiles::of(&cb).0;
    let removed = delete_from_library(&a.handle.root, &a.handle.cache_path, &["uid://same".to_string()]).unwrap();
    assert_eq!(removed, vec!["uid://same".to_string()]);
    assert!(read(&a.handle).assets.is_empty());
    assert!(a_files.iter().all(|f| !a.handle.root.join(f).exists()), "A's files are gone");
    assert!(!a.handle.thumb_path("uid://same").exists());
    assert_eq!(read(&b.handle).assets.len(), 1, "B's cache untouched");
    assert!(read(&b.handle).assets[0].user.tags == vec!["only-b".to_string()]);
    assert!(b_files.iter().all(|f| b.handle.root.join(f).exists()), "B's files remain");
    assert_eq!(fsio::read_bytes(&b.handle.thumb_path("uid://same")).unwrap(), b"png-b");
    let manifest_b: Value = crate::infra::store::read_manifest(&b.handle.root).unwrap();
    assert_eq!(manifest_b["assets"].as_array().unwrap().len(), 1, "B's manifest keeps the asset");
    let _ = &a.data;
    let _ = std::fs::remove_dir_all(&base);
}

struct AssetFiles(Vec<String>);
impl AssetFiles {
    fn of(catalog: &Catalog) -> Self {
        let mut files = Vec::new();
        for asset in &catalog.assets {
            if let crate::domain::catalog_model::AssetContent::Material { material } = &asset.content {
                files.extend(material.maps.iter().map(|m| m.file.clone()));
            }
        }
        AssetFiles(files)
    }
}
