//! A remote library as the app uses it: the merged catalog, files on demand,
//! shared thumbnails and the file cache.

mod common;

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use common::*;
use serde_json::json;
use toybox_core::domain::catalog_model::{Asset, Catalog};
use toybox_core::ops::catalog::{load_catalog_inner, save_user_catalog};
use toybox_remote::client::CatalogReply;
use toybox_server::config::Role;

fn find<'a>(catalog: &'a Catalog, id: &str) -> &'a Asset {
    catalog.assets.iter().find(|asset| asset.id == id).unwrap_or_else(|| panic!("{id} is not in the catalog"))
}

fn ids(catalog: &Catalog) -> Vec<String> {
    let mut ids: Vec<String> = catalog.assets.iter().map(|a| a.id.clone()).collect();
    ids.sort();
    ids
}

fn path_of(root: &std::path::Path, rel: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    path.extend(rel.split('/'));
    path
}

#[test]
fn the_catalog_is_fetched_once_then_kept_with_personal_metadata_merged_on() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("catalog");
    let token = server.token("me", Role::Write);
    let library = world.add(&server, &token, "Remote");
    let handle = library.handle();

    assert!(!toybox_core::infra::store::Store::for_handle(&handle).catalog_exists(), "nothing is cached before the first read");
    let catalog = library.refresh_catalog(false).unwrap();
    assert_eq!(ids(&catalog), vec!["uid://barrel", "uid://click", "uid://crate", "uid://night", "uid://stone"]);
    assert_eq!(library.stats.catalog_downloads.load(Ordering::Relaxed), 1);
    // The root of a remote library is its file cache, and that is what the cache says.
    assert_eq!(catalog.library_root, handle.root_str());
    assert_eq!(handle.root, world.hub.files_root(library.id()));
    assert!(handle.cache_path.starts_with(world.app_data.join("libraries").join(library.id())), "the usual per-library app data");
    assert!(serde_json::to_string(&toybox_core::infra::store::Store::for_handle(&handle).load_catalog().unwrap().unwrap()).unwrap().contains("uid://crate"));

    // Personal metadata is the client's alone.
    let mut mine = load_catalog_inner(&handle).unwrap().unwrap();
    let crate_item = mine.assets.iter_mut().find(|a| a.id == "uid://crate").unwrap();
    crate_item.user.favorite = true;
    crate_item.user.tags.push("mine".into());
    crate_item.user.collections.push("Project".into());
    crate_item.user.preview_asset_id = Some("uid://barrel".into());
    save_user_catalog(&handle, mine).unwrap();

    // Nothing changed on the server: a 304, no download, nothing rewritten.
    let before = toybox_core::infra::store::Store::for_handle(&handle).stamp().unwrap();
    library.refresh_catalog(false).unwrap();
    assert_eq!(library.stats.catalog_downloads.load(Ordering::Relaxed), 1);
    assert_eq!(toybox_core::infra::store::Store::for_handle(&handle).stamp().unwrap(), before);
    // The ETag really is what the server honours.
    let client = library.client();
    let CatalogReply::Fresh { etag: Some(etag), .. } = client.catalog(None).unwrap() else { panic!("a full answer with an ETag") };
    assert!(matches!(client.catalog(Some(&etag)).unwrap(), CatalogReply::NotModified));

    // The server library changes: the new catalog arrives and the client's own data stays.
    server.import(&[("library/_import_x2/sounds/beep.wav", wav())], vec![json!({"id":"uid://beep","kind":"audio","pack":"_import_x2","category":"sounds","file":"beep.wav"})]);
    let catalog = library.refresh_catalog(false).unwrap();
    assert_eq!(library.stats.catalog_downloads.load(Ordering::Relaxed), 2);
    assert_eq!(catalog.assets.len(), 6);
    let kept = find(&catalog, "uid://crate");
    assert!(kept.user.favorite);
    assert!(kept.user.tags.iter().any(|t| t == "mine"), "{:?}", kept.user.tags);
    assert!(kept.user.tags.iter().any(|t| t.contains("Crate Pack")), "tags derived from provenance still arrive: {:?}", kept.user.tags);
    assert_eq!(kept.user.collections, vec!["Project"]);
    assert_eq!(kept.user.preview_asset_id.as_deref(), Some("uid://barrel"));

    // An item that left the server leaves the catalog, the others keep their data.
    toybox_core::ops::delete::delete_from_library(&server.handle().root, &server.handle().cache_path, &["uid://beep".to_string()]).unwrap();
    let catalog = library.refresh_catalog(false).unwrap();
    assert_eq!(catalog.assets.len(), 5);
    assert!(find(&catalog, "uid://crate").user.favorite);

    // The server never sees any of it.
    let CatalogReply::Fresh { body, .. } = client.catalog(None).unwrap() else { panic!() };
    let served: Catalog = serde_json::from_slice(&body).unwrap();
    assert!(served.assets.iter().all(|a| !a.user.favorite && a.user.collections.is_empty() && !a.user.tags.iter().any(|t| t == "mine")));
    assert!(served.library_root.is_empty());

    // A forced refresh downloads again even though nothing changed.
    library.refresh_catalog(true).unwrap();
    assert_eq!(library.stats.catalog_downloads.load(Ordering::Relaxed), 4);
    assert!(find(&load_catalog_inner(&handle).unwrap().unwrap(), "uid://crate").user.favorite);
}

#[test]
fn files_arrive_when_first_needed_and_a_model_brings_everything_it_needs() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("files");
    let library = world.add_loaded(&server, &server.token("me", Role::Read), "Remote");
    let handle = library.handle();
    let catalog = load_catalog_inner(&handle).unwrap().unwrap();
    let crate_item = find(&catalog, "uid://crate");
    let fileset = crate_item.fileset().unwrap();
    assert_eq!(fileset.textures.len(), 2);

    assert!(!path_of(&handle.root, &fileset.gltf).exists(), "nothing is downloaded by reading the catalog");
    // Asking for the glTF (what the viewer does first) brings its buffer and both images as well.
    handle.ensure_files(&[fileset.gltf.clone()]).unwrap();
    for rel in [&fileset.gltf, &fileset.bin, &fileset.textures[0], &fileset.textures[1]] {
        let local = path_of(&handle.root, rel);
        assert_eq!(std::fs::read(&local).unwrap(), std::fs::read(path_of(&server.library_root(), rel)).unwrap(), "{rel}");
    }
    assert_eq!(library.stats.file_downloads.load(Ordering::Relaxed), 4, "gltf, bin and two images, in one go");
    // The variant has its own files and they were not fetched.
    let variant = &crate_item.variants[0];
    assert!(!path_of(&handle.root, &variant.fileset().unwrap().gltf).exists());
    // Asking again fetches nothing.
    handle.ensure_files(&[fileset.textures[1].clone()]).unwrap();
    handle.ensure_assets(&[find(&catalog, "uid://barrel").clone()]).unwrap();
    assert_eq!(library.stats.file_downloads.load(Ordering::Relaxed), 4 + 2, "barrel adds its glTF and buffer; the shared image is already there");

    // Whole items bring their variants, but not the original files kept with them; those are asked for by path.
    handle.ensure_assets(&[crate_item.clone()]).unwrap();
    assert!(path_of(&handle.root, &variant.fileset().unwrap().gltf).exists());
    let origin = crate_item.origin.as_ref().unwrap();
    assert!(origin.files.iter().all(|rel| !path_of(&handle.root, rel).exists()), "kept original files are not downloaded for viewing");
    handle.ensure_files(&origin.files).unwrap();
    assert!(origin.files.iter().all(|rel| path_of(&handle.root, rel).exists()));

    // Anything that reads files by path through core is covered: it fetches first.
    let stone = find(&catalog, "uid://stone");
    let AssetContentMaterial(maps) = AssetContentMaterial::of(stone);
    assert!(maps.iter().all(|rel| !path_of(&handle.root, rel).exists()));
    let out = world.dir.join("export");
    let written = toybox_core::ops::export_plugin::perform_asset_copy(&handle, "uid://stone", &out.to_string_lossy(), "Stone", false).unwrap();
    assert!(!written.is_empty() && written.iter().all(|rel| path_of(&out, rel).exists()));
    assert!(maps.iter().all(|rel| path_of(&handle.root, rel).exists()));
    let doc = toybox_core::ops::export_plugin::read_asset_gltf(&handle, "uid://barrel").unwrap();
    assert!(doc["images"].is_array());
    let fingerprints = toybox_core::ops::export_plugin::asset_fingerprints(&handle, &["uid://click".to_string(), "uid://night".to_string()], false).unwrap();
    assert_eq!(fingerprints.len(), 2);

    // A path that is not a library file is refused before any request is made.
    assert!(handle.ensure_files(&["../../etc/passwd".to_string()]).is_err());
    assert!(handle.ensure_files(&["_library_config/catalog.json".to_string()]).is_err());
    // And a file the server does not have is a plain error.
    let error = handle.ensure_files(&["library/models/0123456789abcdef/missing.png".to_string()]).unwrap_err().to_string();
    assert!(error.contains("no such file"), "{error}");
}

struct AssetContentMaterial(Vec<String>);
impl AssetContentMaterial {
    fn of(asset: &Asset) -> Self {
        let mut files = Vec::new();
        asset.content_files(&mut files);
        AssetContentMaterial(files.into_iter().filter(|f| f.ends_with("color.png")).collect())
    }
}

#[test]
fn many_requests_for_one_file_download_it_once() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("concurrent");
    let library = world.add_loaded(&server, &server.token("me", Role::Read), "Remote");
    let handle = library.handle();
    let catalog = load_catalog_inner(&handle).unwrap().unwrap();
    let fileset = find(&catalog, "uid://crate").fileset().unwrap().clone();

    let gate = std::sync::Barrier::new(12);
    std::thread::scope(|scope| {
        for n in 0..12 {
            let handle = handle.clone();
            let rel = if n % 2 == 0 { fileset.textures[0].clone() } else { fileset.gltf.clone() };
            let gate = &gate;
            scope.spawn(move || {
                gate.wait();
                handle.ensure_files(&[rel]).expect("every waiting request succeeds");
            });
        }
    });
    assert_eq!(library.stats.file_downloads.load(Ordering::Relaxed), 4, "each of the four files was fetched exactly once");
    // No half-written files are left, and the ones there are whole.
    fn all_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() { all_files(&path, out) } else { out.push(path) }
        }
    }
    let mut files = Vec::new();
    all_files(&handle.root, &mut files);
    assert_eq!(files.len(), 4);
    assert!(files.iter().all(|f| f.extension().is_none_or(|e| e != "part")));
    for rel in [&fileset.gltf, &fileset.bin, &fileset.textures[0], &fileset.textures[1]] {
        assert_eq!(std::fs::read(path_of(&handle.root, rel)).unwrap(), std::fs::read(path_of(&server.library_root(), rel)).unwrap());
    }
}

#[test]
fn shared_thumbnails_are_fetched_lazily_and_ours_are_offered_only_with_a_write_token() {
    let server = TestServer::start();
    server.seed();
    toybox_core::ops::thumbs::save_thumb(&server.handle(), "uid://crate", &png()).unwrap();
    let mut world = World::new("thumbs");
    let library = world.add(&server, &server.token("writer", Role::Write), "Writer");
    let catalog = library.refresh_catalog(false).unwrap();
    let handle = library.handle();

    // The server's picture counts as ready without being downloaded; the rest still need rendering.
    assert_eq!(library.shared_thumb_count(), 1);
    assert!(handle.has_thumb("uid://crate") && !handle.thumb_path("uid://crate").exists());
    assert_eq!(format!("{:?}", find(&catalog, "uid://crate").thumb.state), "Ready");
    assert_eq!(format!("{:?}", find(&catalog, "uid://barrel").thumb.state), "Missing");
    let pending = toybox_core::ops::thumbs::list_pending_thumbs(&handle).unwrap();
    assert!(!pending.contains(&"uid://crate".to_string()) && pending.contains(&"uid://barrel".to_string()), "{pending:?}");

    // Asking for the picture downloads it, once.
    let link = handle.remote.as_ref().unwrap();
    assert!(link.files.ensure_thumb("uid://crate").unwrap());
    assert_eq!(std::fs::read(handle.thumb_path("uid://crate")).unwrap(), png());
    assert!(link.files.ensure_thumb("uid://crate").unwrap());
    assert_eq!(library.stats.thumb_downloads.load(Ordering::Relaxed), 1);
    assert!(!link.files.ensure_thumb("uid://barrel").unwrap(), "the server has none for it");

    // A picture rendered here is offered to the server.
    let mine = [png(), b"barrel".to_vec()].concat();
    toybox_core::ops::thumbs::save_thumb(&handle, "uid://barrel", &mine).unwrap();
    library.flush_thumb_uploads();
    assert_eq!(library.stats.thumb_uploads.load(Ordering::Relaxed), 1);
    assert_eq!(std::fs::read(server.handle().thumb_path("uid://barrel")).unwrap(), mine);
    assert!(library.client().thumbs(None).is_ok());

    // The server replaces the first picture: this client drops its copy and takes the new one.
    let newer = [png(), b"new crate picture".to_vec()].concat();
    std::thread::sleep(std::time::Duration::from_millis(20));
    // Through the server's own door: pictures changed behind its back are not in its thumbnail log.
    library.client().put_thumb("uid://crate", &newer).unwrap();
    library.refresh_catalog(false).unwrap();
    assert!(!handle.thumb_path("uid://crate").exists());
    assert!(link.files.ensure_thumb("uid://crate").unwrap());
    assert_eq!(std::fs::read(handle.thumb_path("uid://crate")).unwrap(), newer);
    // Our own upload is not fetched back.
    assert_eq!(std::fs::read(handle.thumb_path("uid://barrel")).unwrap(), mine);
    assert_eq!(library.stats.thumb_downloads.load(Ordering::Relaxed), 2);

    // A read-only token never uploads, and does not complain.
    let reader = world.add_loaded(&server, &server.token("reader", Role::Read), "Reader");
    let read_handle = reader.handle();
    toybox_core::ops::thumbs::save_thumb(&read_handle, "uid://click", &png()).unwrap();
    reader.flush_thumb_uploads();
    assert_eq!(reader.stats.thumb_uploads.load(Ordering::Relaxed), 0);
    assert!(!server.handle().thumb_path("uid://click").exists());
    assert!(read_handle.thumb_path("uid://click").exists(), "it is still saved locally");
    // Something that is not a PNG is not offered either.
    toybox_core::ops::thumbs::save_thumb(&handle, "uid://night", b"not a png").unwrap();
    library.flush_thumb_uploads();
    assert!(!server.handle().thumb_path("uid://night").exists());
}

#[test]
fn downloaded_files_can_be_measured_and_cleared_without_touching_anything_else() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("cache");
    let library = world.add_loaded(&server, &server.token("me", Role::Write), "Remote");
    let handle = library.handle();
    assert_eq!(library.cache_size(), 0);
    let catalog = load_catalog_inner(&handle).unwrap().unwrap();
    handle.ensure_assets(&[find(&catalog, "uid://crate").clone(), find(&catalog, "uid://click").clone()]).unwrap();
    let size = library.cache_size();
    assert!(size > 0);
    let expected: u64 = {
        let mut files = Vec::new();
        find(&catalog, "uid://crate").collect_content_files(&mut files);
        find(&catalog, "uid://click").collect_content_files(&mut files);
        files.sort();
        files.dedup();
        files.iter().map(|rel| std::fs::metadata(path_of(&server.library_root(), rel)).unwrap().len()).sum()
    };
    assert_eq!(size, expected);

    // A staged import is not a download: it is neither counted nor cleared.
    let staged = handle.root.join("library/_import_wip/props/a.bin");
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::write(&staged, b"staged").unwrap();
    assert_eq!(library.cache_size(), size);
    assert_eq!(library.clear_cache(), size);
    assert_eq!(library.cache_size(), 0);
    assert!(staged.exists());
    assert!(toybox_core::infra::store::Store::for_handle(&handle).catalog_exists(), "the catalog (and the favourites in it) stay");
    // Files simply come back when needed.
    handle.ensure_assets(&[find(&catalog, "uid://click").clone()]).unwrap();
    assert!(library.cache_size() > 0);
}

#[test]
fn asking_for_an_image_two_items_share_fetches_only_that_image() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("shared-image");
    let library = world.add_loaded(&server, &server.token("me", Role::Read), "Remote");
    let handle = library.handle();
    let catalog = load_catalog_inner(&handle).unwrap().unwrap();
    let shared = find(&catalog, "uid://barrel").fileset().unwrap().textures[0].clone();
    assert!(find(&catalog, "uid://crate").fileset().unwrap().textures.contains(&shared), "the barrel and the crate use one image");
    handle.ensure_files(&[shared.clone()]).unwrap();
    assert_eq!(library.stats.file_downloads.load(Ordering::Relaxed), 1);
    assert!(path_of(&handle.root, &shared).exists());
    assert!(!path_of(&handle.root, &find(&catalog, "uid://barrel").fileset().unwrap().gltf).exists());
    // The crate's other image is shared with its detail-level variant, so it also stands alone.
    let other = find(&catalog, "uid://crate").fileset().unwrap().textures.iter().find(|t| **t != shared).unwrap().clone();
    handle.ensure_files(&[other]).unwrap();
    assert_eq!(library.stats.file_downloads.load(Ordering::Relaxed), 2);
    // A model's own file brings the model whole, shared images included.
    handle.ensure_files(&[find(&catalog, "uid://barrel").fileset().unwrap().bin.clone()]).unwrap();
    assert!(path_of(&handle.root, &find(&catalog, "uid://barrel").fileset().unwrap().gltf).exists());
    assert_eq!(library.stats.file_downloads.load(Ordering::Relaxed), 4);
}

#[test]
fn content_fingerprints_from_the_server_are_kept_with_the_catalog_and_arrive_when_the_server_adds_them() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("fingerprints");
    let token = server.token("me", Role::Write);
    let library = world.add(&server, &token, "Remote");
    let handle = library.handle();
    let catalog = library.refresh_catalog(false).unwrap();

    // Every item and variant carries what the server computed, and the client does not recompute or drop it.
    let served = server.catalog();
    let mut seen = 0;
    for asset in &catalog.assets {
        let theirs = find(&served, &asset.id);
        let hash = asset.content_hash.as_deref().unwrap_or_else(|| panic!("{} has no contentHash", asset.id));
        assert_eq!(Some(hash), theirs.content_hash.as_deref());
        assert_eq!(hash.len(), 64);
        for (variant, their_variant) in asset.variants.iter().zip(&theirs.variants) {
            assert!(variant.content_hash.is_some() && variant.content_hash == their_variant.content_hash, "{}", variant.id);
            seen += 1;
        }
        seen += 1;
    }
    assert!(seen >= 7, "items and variants: {seen}");
    // They are in the cached catalog (so they are there offline too) and survive the client saving its own metadata.
    let mut cached = load_catalog_inner(&handle).unwrap().unwrap();
    assert!(cached.assets.iter().all(|a| a.content_hash.is_some()));
    cached.assets.iter_mut().find(|a| a.id == "uid://crate").unwrap().user.favorite = true;
    save_user_catalog(&handle, cached).unwrap();
    let cached = load_catalog_inner(&handle).unwrap().unwrap();
    assert!(cached.assets.iter().all(|a| a.content_hash.is_some()) && find(&cached, "uid://crate").user.favorite);

    // The server restarts with no fingerprints (an old library): the client simply has none to use.
    toybox_core::infra::store::hashes::update(&server.handle().library_db_path(), |map| map.clear()).unwrap();
    let server = server.restart();
    let catalog = library.refresh_catalog(false).unwrap();
    assert!(catalog.assets.iter().all(|a| a.content_hash.is_none()), "nothing known, so the fallback rule applies");
    // The server fingerprints in the background; its ETag moved, so the next refresh downloads the catalog again.
    let downloads = library.stats.catalog_downloads.load(Ordering::Relaxed);
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let report = runtime.block_on(server.state.library.fingerprint_pass()).unwrap();
    assert!(report.hashed >= 5, "{report:?}");
    let catalog = library.refresh_catalog(false).unwrap();
    assert_eq!(library.stats.catalog_downloads.load(Ordering::Relaxed), downloads + 1);
    assert!(catalog.assets.iter().all(|a| a.content_hash.is_some()));
    assert_eq!(find(&catalog, "uid://crate").content_hash, find(&server.catalog(), "uid://crate").content_hash);
}
