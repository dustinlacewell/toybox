//! The content fingerprint: what it ignores, what it does not, and how the side
//! cache and the backfill behave. Libraries are built through the real import
//! path (`merge_entries`), so files are stored exactly as an import stores them.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::catalog::{load_current_catalog, save_catalog_inner};
use super::content_hash::*;
use super::import::merge_entries;
use crate::domain::catalog_model::{Asset, Catalog};
use crate::infra::fsio;
use crate::infra::registry::{LibraryEntry, LibraryHandle};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Base(PathBuf);

impl Base {
    fn new(name: &str) -> Base {
        let dir = std::env::temp_dir().join(format!("toybox-content-hash-{name}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        Base(dir)
    }
}

impl Drop for Base {
    fn drop(&mut self) {
        // Only the folder this test made.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn library(base: &Base, id: &str) -> LibraryHandle {
    let root = base.0.join(format!("root-{id}"));
    crate::infra::library::ensure_library_folder(&root).unwrap();
    let root = root.canonicalize().unwrap();
    LibraryHandle::local(&base.0.join("appdata"), &LibraryEntry::local(id, id, &root.to_string_lossy()))
}

fn put(library: &LibraryHandle, rel: &str, bytes: &[u8]) {
    fsio::write_bytes(&library.root.join(rel), bytes).unwrap();
}

/// Empty the side cache (it used to be a file to delete; it is a table now).
fn forget_hashes(file: &std::path::Path) {
    crate::infra::store::hashes::update(file, |map| map.clear()).unwrap();
}

/// The scheme the side cache is stamped with.
fn scheme_of(file: &std::path::Path) -> u32 {
    let connection = rusqlite::Connection::open(file).unwrap();
    connection.query_row("SELECT value FROM meta WHERE key='hashScheme'", [], |row| row.get::<_, String>(0)).unwrap().parse().unwrap()
}

fn stamp_scheme(file: &std::path::Path, scheme: u32) {
    let connection = rusqlite::Connection::open(file).unwrap();
    connection.execute("UPDATE meta SET value=?1 WHERE key='hashScheme'", [scheme.to_string()]).unwrap();
}

fn wav(fill: u8) -> Vec<u8> {
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
    bytes.extend(vec![fill; data as usize]);
    bytes
}

const PIXEL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7w==";

/// Stage and commit a model: `<stem>.gltf` (+ `<stem>.bin`) with images named
/// `images` (their bytes are `image_bytes`). `uris` are written into the glTF as given.
fn import_model(library: &LibraryHandle, tag: &str, id: &str, stem: &str, category: &str, bin: &[u8], images: &[(&str, &[u8])]) {
    let pack = format!("_import_{tag}");
    let uris: Vec<Value> = images.iter().map(|(name, _)| json!({"uri": name})).collect();
    let scene = json!({"asset":{"version":"2.0"},"buffers":[{"uri":format!("{stem}.bin"),"byteLength":bin.len()}],"images":uris});
    put(library, &format!("library/{pack}/{category}/{stem}.gltf"), &serde_json::to_vec(&scene).unwrap());
    put(library, &format!("library/{pack}/{category}/{stem}.bin"), bin);
    for (name, bytes) in images {
        put(library, &format!("library/{pack}/{category}/{name}"), bytes);
    }
    merge_entries(library, vec![json!({"id":id,"pack":pack,"category":category,"file":format!("{stem}.gltf")})]).unwrap();
}

fn catalog(library: &LibraryHandle) -> Catalog {
    load_current_catalog(library).unwrap().expect("catalog")
}

fn find<'a>(catalog: &'a Catalog, id: &str) -> &'a Asset {
    catalog.assets.iter().find(|a| a.id == id).unwrap_or_else(|| panic!("no {id}"))
}

/// The hash of `id` computed from scratch (no side cache).
fn computed(library: &LibraryHandle, id: &str) -> String {
    let catalog = catalog(library);
    ContentHasher::new(&library.root).hash_asset(find(&catalog, id)).unwrap()
}

fn is_hex_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[test]
fn the_same_model_imported_under_other_names_ids_packs_and_files_has_the_same_hash() {
    let base = Base::new("model-names");
    let (a, b) = (library(&base, "a"), library(&base, "b"));
    import_model(&a, "one", "uid://crate", "Crate", "props", b"mesh-bytes", &[("wood.png", b"wood-pixels"), ("moss.png", b"moss-pixels")]);
    // Another id, file stem, pack, category, image names (and order of nothing else).
    import_model(&b, "two", "uid://box_17", "WoodenBox", "containers", b"mesh-bytes", &[("tex_a.png", b"wood-pixels"), ("tex_b.png", b"moss-pixels")]);
    let (ha, hb) = (computed(&a, "uid://crate"), computed(&b, "uid://box_17"));
    assert!(is_hex_sha256(&ha), "{ha}");
    assert_eq!(ha, hb, "names, ids, packs and categories are not content");

    // Content is: a changed buffer, a changed image or a swapped image changes it.
    import_model(&b, "three", "uid://box_bin", "Other", "containers", b"mesh-BYTES", &[("a.png", b"wood-pixels"), ("b.png", b"moss-pixels")]);
    import_model(&b, "four", "uid://box_img", "Third", "containers", b"mesh-bytes", &[("a.png", b"wood-pixels"), ("b.png", b"MOSS-pixels")]);
    import_model(&b, "five", "uid://box_swap", "Fourth", "containers", b"mesh-bytes", &[("a.png", b"moss-pixels"), ("b.png", b"wood-pixels")]);
    for id in ["uid://box_bin", "uid://box_img", "uid://box_swap"] {
        assert_ne!(computed(&b, id), ha, "{id}");
    }
}

#[test]
fn what_an_import_writes_into_the_stored_scene_does_not_make_two_copies_differ() {
    let base = Base::new("model-stored");
    let (a, b) = (library(&base, "a"), library(&base, "b"));
    import_model(&a, "one", "uid://x", "Alpha", "props", b"bin", &[]);
    import_model(&b, "two", "uid://y", "Beta_Renamed", "other", b"bin", &[]);
    // The stored scenes really do differ: each records the import's staging path.
    let path = |library: &LibraryHandle, id: &str| library.root.join(find(&catalog(library), id).fileset().unwrap().gltf.clone());
    let (text_a, text_b) = (std::fs::read_to_string(path(&a, "uid://x")).unwrap(), std::fs::read_to_string(path(&b, "uid://y")).unwrap());
    assert!(text_a.contains("toyboxSourcePath") && text_a.contains("_import_one") && text_b.contains("_import_two"), "{text_a}");
    assert_ne!(text_a, text_b);
    assert_eq!(computed(&a, "uid://x"), computed(&b, "uid://y"));
}

#[test]
fn embedded_images_stay_part_of_the_scene() {
    let base = Base::new("model-data");
    let (a, b) = (library(&base, "a"), library(&base, "b"));
    for (library, tag, id, stem, uri) in [(&a, "one", "uid://p", "Pa", PIXEL), (&b, "two", "uid://q", "Qb", PIXEL), (&b, "three", "uid://r", "Rc", "data:image/png;base64,AAAA")] {
        let pack = format!("_import_{tag}");
        let scene = json!({"asset":{"version":"2.0"},"buffers":[{"uri":format!("{stem}.bin"),"byteLength":3}],"images":[{"uri":uri}]});
        put(library, &format!("library/{pack}/props/{stem}.gltf"), &serde_json::to_vec(&scene).unwrap());
        put(library, &format!("library/{pack}/props/{stem}.bin"), b"bin");
        merge_entries(library, vec![json!({"id":id,"pack":pack,"category":"props","file":format!("{stem}.gltf")})]).unwrap();
    }
    let same = computed(&a, "uid://p");
    assert_eq!(same, computed(&b, "uid://q"));
    assert_ne!(same, computed(&b, "uid://r"), "another embedded image is another model");
}

#[test]
fn a_model_and_its_variants_each_have_their_own_hash_and_kept_originals_do_not_count() {
    let base = Base::new("variants");
    let (a, b) = (library(&base, "a"), library(&base, "b"));
    for (library, tag, original) in [(&a, "one", "original A"), (&b, "two", "a different original")] {
        let pack = format!("_import_{tag}");
        for (stem, bin) in [("Rock", &b"full"[..]), ("Rock_LOD", &b"low"[..])] {
            put(library, &format!("library/{pack}/props/{stem}.gltf"), &serde_json::to_vec(&json!({"asset":{"version":"2.0"},"buffers":[{"uri":format!("{stem}.bin"),"byteLength":4}]})).unwrap());
            put(library, &format!("library/{pack}/props/{stem}.bin"), bin);
            put(library, &format!("library/{pack}/props/{stem}.fbx"), original.as_bytes());
        }
        let origin = |stem: &str| json!({"plugin":"x","files":[format!("{stem}.fbx")]});
        merge_entries(library, vec![json!({"id":"uid://rock","pack":pack,"category":"props","file":"Rock.gltf","origin":origin("Rock"),
            "variants":[{"id":"uid://rock_lod","pack":pack,"category":"props","file":"Rock_LOD.gltf","origin":origin("Rock_LOD")}]})]).unwrap();
    }
    let (ca, cb) = (catalog(&a), catalog(&b));
    let (top_a, top_b) = (find(&ca, "uid://rock"), find(&cb, "uid://rock"));
    let mut hasher_a = ContentHasher::new(&a.root);
    let mut hasher_b = ContentHasher::new(&b.root);
    let (full_a, low_a) = (hasher_a.hash_asset(top_a).unwrap(), hasher_a.hash_asset(&top_a.variants[0]).unwrap());
    assert_ne!(full_a, low_a, "a variant is its own content");
    assert_eq!(full_a, hasher_b.hash_asset(top_b).unwrap(), "the kept original is not part of the content");
    assert_eq!(low_a, hasher_b.hash_asset(&top_b.variants[0]).unwrap());
}

#[test]
fn materials_ignore_order_file_names_and_kept_archives_but_not_what_changes_the_look() {
    let base = Base::new("material");
    let (a, b) = (library(&base, "a"), library(&base, "b"));
    let stage = |library: &LibraryHandle, tag: &str, files: &[(&str, &[u8])]| {
        for (name, bytes) in files {
            put(library, &format!("library/_import_{tag}/materials/Stone/{name}"), bytes);
        }
    };
    let entry = |tag: &str, id: &str, maps: Value, extra: Value| {
        let mut entry = json!({"id":id,"kind":"material","pack":format!("_import_{tag}"),"category":"stone","name":"Stone","maps":maps});
        for (key, value) in extra.as_object().unwrap() {
            entry[key] = value.clone();
        }
        entry
    };
    stage(&a, "one", &[("color.png", b"C"), ("orm.png", b"P"), ("n.png", b"N"), ("stone.sbsar", b"archive A")]);
    stage(&b, "two", &[("base_color.png", b"C"), ("packed.png", b"P"), ("normal.png", b"N"), ("other.sbsar", b"different archive")]);
    merge_entries(&a, vec![entry("one", "uid://m", json!([{"role":"albedo","file":"color.png"},{"role":"roughness","file":"orm.png","channel":"g"},{"role":"normal","file":"n.png"}]),
        json!({"origin":{"plugin":"s","files":["stone.sbsar"]}}))]).unwrap();
    // Same maps, other order, other file names, other id, other archive.
    merge_entries(&b, vec![entry("two", "uid://stone_b", json!([{"role":"normal","file":"normal.png"},{"role":"roughness","file":"packed.png","channel":"g"},{"role":"albedo","file":"base_color.png"}]),
        json!({"origin":{"plugin":"s","files":["other.sbsar"]}}))]).unwrap();
    let reference = computed(&a, "uid://m");
    assert_eq!(reference, computed(&b, "uid://stone_b"));

    // A different channel, an inverted map, a flipped normal or another image each make another material.
    let variants = [
        ("uid://channel", json!([{"role":"albedo","file":"base_color.png"},{"role":"roughness","file":"packed.png","channel":"b"},{"role":"normal","file":"normal.png"}]), json!({})),
        ("uid://invert", json!([{"role":"albedo","file":"base_color.png"},{"role":"roughness","file":"packed.png","channel":"g","invert":true},{"role":"normal","file":"normal.png"}]), json!({})),
        ("uid://flip", json!([{"role":"albedo","file":"base_color.png"},{"role":"roughness","file":"packed.png","channel":"g"},{"role":"normal","file":"normal.png"}]), json!({"flipNormalGreen":true})),
        ("uid://role", json!([{"role":"albedo","file":"base_color.png"},{"role":"metallic","file":"packed.png","channel":"g"},{"role":"normal","file":"normal.png"}]), json!({})),
    ];
    for (id, maps, extra) in variants {
        stage(&b, "three", &[("base_color.png", b"C"), ("packed.png", b"P"), ("normal.png", b"N")]);
        merge_entries(&b, vec![entry("three", id, maps, extra)]).unwrap();
        assert_ne!(computed(&b, id), reference, "{id}");
    }
}

#[test]
fn audio_and_plugin_types_hash_their_files_and_the_type_is_part_of_it() {
    let base = Base::new("audio-plugin");
    let (a, b) = (library(&base, "a"), library(&base, "b"));
    put(&a, "library/_import_one/sfx/click.wav", &wav(1));
    put(&b, "library/_import_two/ui/Button Press.wav", &wav(1));
    put(&b, "library/_import_two/ui/Other.wav", &wav(2));
    merge_entries(&a, vec![json!({"id":"uid://click","kind":"audio","pack":"_import_one","category":"sfx","file":"click.wav"})]).unwrap();
    merge_entries(&b, vec![
        json!({"id":"uid://press","kind":"audio","pack":"_import_two","category":"ui","file":"Button Press.wav"}),
        json!({"id":"uid://other","kind":"audio","pack":"_import_two","category":"ui","file":"Other.wav"}),
    ]).unwrap();
    assert_eq!(computed(&a, "uid://click"), computed(&b, "uid://press"));
    assert_ne!(computed(&a, "uid://click"), computed(&b, "uid://other"));

    // Plugin-defined items: the files in order; file names do not count.
    for (library, tag, names) in [(&a, "p1", ["px.png", "nx.png"]), (&b, "p2", ["right.png", "left.png"])] {
        put(library, &format!("library/_import_{tag}/skybox/Night/{}", names[0]), b"PX");
        put(library, &format!("library/_import_{tag}/skybox/Night/{}", names[1]), b"NX");
        merge_entries(library, vec![json!({"id":"uid://night","kind":"plugin","type":"skybox","pack":format!("_import_{tag}"),"category":"night","name":"Night",
            "files":names})]).unwrap();
    }
    assert_eq!(computed(&a, "uid://night"), computed(&b, "uid://night"));
    // The same files in another order are another item; so is the same bytes under another type.
    put(&b, "library/_import_p3/skybox/Night/a.png", b"NX");
    put(&b, "library/_import_p3/skybox/Night/b.png", b"PX");
    merge_entries(&b, vec![json!({"id":"uid://swapped","kind":"plugin","type":"skybox","pack":"_import_p3","category":"night","name":"Night","files":["a.png","b.png"]})]).unwrap();
    assert_ne!(computed(&b, "uid://swapped"), computed(&a, "uid://night"));
    put(&b, "library/_import_p4/decal/Night/a.png", b"PX");
    put(&b, "library/_import_p4/decal/Night/b.png", b"NX");
    merge_entries(&b, vec![json!({"id":"uid://elsewhere","kind":"plugin","type":"decal","pack":"_import_p4","category":"night","name":"Night","files":["a.png","b.png"]})]).unwrap();
    assert_ne!(computed(&b, "uid://elsewhere"), computed(&a, "uid://night"), "the type is in the preimage");
}

#[test]
fn stored_identity_records_of_a_scene_are_normalised_and_nothing_else() {
    let base = Base::new("identity");
    let root = base.0.join("root");
    let write = |name: &str, doc: Value| {
        let path = root.join("library/animations/0123456789abcdef").join(name);
        fsio::write_bytes(&path, &serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
        format!("library/animations/0123456789abcdef/{name}")
    };
    let clip = |id: &str, source: &str, source_name: &str, path: &str, walk: &str| json!({
        "asset":{"version":"2.0"},"animations":[{"name":walk}],
        "extras":{"toyboxSourcePath":path,"toyboxAnimation":{"id":id,"clip":{"sourceId":source,"sourceName":source_name,"index":0,"name":"Walk"},"joints":["a","b"]}}
    });
    let mut hasher = ContentHasher::new(&root);
    let one = write("one.gltf", clip("uid://anim_1", "uid://hero", "Hero", "library/_import_x/anims/Hero.gltf", "Walk"));
    let two = write("two.gltf", clip("uid://anim_2", "uid://hero_copy", "Hero Copy", "library/_import_y/other/Copy.gltf", "Walk"));
    let renamed = write("three.gltf", clip("uid://anim_1", "uid://hero", "Hero", "library/_import_x/anims/Hero.gltf", "Run"));
    let reference = hasher.gltf_digest(&one).unwrap();
    assert_eq!(reference, hasher.gltf_digest(&two).unwrap(), "ids, source names and staging paths are not content");
    assert_ne!(reference, hasher.gltf_digest(&renamed).unwrap(), "the clip's own name is");
    // Joints (the rig) are content too.
    let mut other_rig = clip("uid://anim_1", "uid://hero", "Hero", "p", "Walk");
    other_rig["extras"]["toyboxAnimation"]["joints"] = json!(["a", "c"]);
    assert_ne!(reference, hasher.gltf_digest(&write("four.gltf", other_rig)).unwrap());
    // The extension form of the source path is normalised the same way, and an object left empty by it is dropped.
    let with_extension = write("five.gltf", json!({"asset":{"version":"2.0"},"extensions":{"TOYBOX_library":{"sourcePath":"library/_import_z/a.gltf"}}}));
    let without = write("six.gltf", json!({"asset":{"version":"2.0"}}));
    assert_eq!(hasher.gltf_digest(&with_extension).unwrap(), hasher.gltf_digest(&without).unwrap());
    let kept = write("seven.gltf", json!({"asset":{"version":"2.0"},"extras":{"author":"me","toyboxSourcePath":"p"}}));
    let kept_other = write("eight.gltf", json!({"asset":{"version":"2.0"},"extras":{"author":"you","toyboxSourcePath":"p"}}));
    assert_ne!(hasher.gltf_digest(&kept).unwrap(), hasher.gltf_digest(&kept_other).unwrap(), "other extras stay");
}

#[test]
fn files_are_found_by_their_uri_with_percent_escapes_and_unsafe_names_are_refused() {
    let base = Base::new("uri");
    let root = base.0.join("root");
    let folder = "library/models/0123456789abcdef";
    fsio::write_bytes(&root.join(folder).join("My Tex.png"), b"pixels").unwrap();
    fsio::write_bytes(&root.join(folder).join("plain.png"), b"pixels").unwrap();
    let scene = |uri: &str| {
        let path = format!("{folder}/{}.gltf", uri.len());
        fsio::write_bytes(&root.join(&path), &serde_json::to_vec(&json!({"images":[{"uri":uri}]})).unwrap()).unwrap();
        path
    };
    let mut hasher = ContentHasher::new(&root);
    let (escaped, plain) = (scene("My%20Tex.png"), scene("plain.png"));
    assert_eq!(hasher.gltf_digest(&escaped).unwrap(), hasher.gltf_digest(&plain).unwrap(), "the file is found through its escaped name, and only its bytes count");
    for bad in ["/etc/passwd", "C:\\x.png", "..\\x.png", "../../../outside.png"] {
        assert!(hasher.gltf_digest(&scene(bad)).is_err(), "{bad}");
    }
    assert!(hasher.gltf_digest(&scene("missing.png")).is_err());
    assert!(hasher.file_digest("../x").is_err() && hasher.file_digest("library/../x").is_err() && hasher.file_digest("other/x").is_err());
}

#[test]
fn a_shared_image_is_read_once_per_run() {
    let base = Base::new("shared");
    let library = library(&base, "a");
    import_model(&library, "one", "uid://one", "One", "props", b"1", &[("shared.png", b"the shared texture")]);
    import_model(&library, "two", "uid://two", "Two", "props", b"2", &[("shared.png", b"the shared texture")]);
    let catalog = catalog(&library);
    let mut hasher = ContentHasher::new(&library.root);
    hasher.hash_asset(find(&catalog, "uid://one")).unwrap();
    // The image lives once in `_resources`; take it away: the second model only hashes because the first read was remembered.
    let resources = library.root.join("library/models/_resources");
    let files: Vec<_> = std::fs::read_dir(&resources).unwrap().flatten().collect();
    assert_eq!(files.len(), 1);
    std::fs::remove_file(files[0].path()).unwrap();
    assert!(hasher.hash_asset(find(&catalog, "uid://two")).is_ok());
    assert!(ContentHasher::new(&library.root).hash_asset(find(&catalog, "uid://two")).is_err(), "a fresh hasher has to read it");
}

#[test]
fn the_side_cache_keys_are_storage_folders_and_ignore_unmanaged_paths() {
    let base = Base::new("keys");
    let library = library(&base, "a");
    import_model(&library, "one", "uid://one", "One", "props", b"1", &[]);
    let catalog = catalog(&library);
    let asset = find(&catalog, "uid://one");
    let key = storage_key(asset).unwrap();
    let gltf = &asset.fileset().unwrap().gltf;
    assert!(key.starts_with("models/") && gltf.starts_with(&format!("library/{key}/")), "{key} / {gltf}");
    let mut loose = asset.clone();
    if let crate::domain::catalog_model::AssetContent::Model { fileset, .. } = &mut loose.content {
        fileset.gltf = "library/pack/category/One.gltf".into();
    }
    assert_eq!(storage_key(&loose), None, "a folder that is not content addressed is not cached");
}

#[test]
fn backfill_resumes_reports_progress_and_never_rereads_what_is_saved() {
    let base = Base::new("backfill");
    let library = library(&base, "a");
    // Build a library whose cache knows nothing: import, then remove the side cache.
    for i in 0..7 {
        import_model(&library, &format!("t{i}"), &format!("uid://m{i}"), &format!("M{i}"), "props", format!("bin{i}").as_bytes(), &[]);
    }
    let file = cache_path(&library);
    forget_hashes(&file);
    let catalog = catalog(&library);
    assert_eq!(count_unhashed(&catalog), 7);
    let cancel = AtomicBool::new(false);
    let skip = HashSet::new();

    // First pass: three items, saved every two.
    let mut progress = Vec::new();
    let mut saved = Vec::new();
    let report = backfill(&library.root, &file, &catalog, &cancel, &BackfillOptions { save_every: 2, max_items: Some(3), skip: &skip, ..Default::default() }, &mut |event| match event {
        Event::Started { total } => progress.push((0, total)),
        Event::Progress { done, total } => progress.push((done, total)),
        Event::Saved { hashes } => saved.push(hashes.len()),
    })
    .unwrap();
    assert_eq!((report.total, report.attempted, report.hashed, report.failed.len(), report.cancelled), (7, 3, 3, 0, false));
    assert_eq!(progress.first(), Some(&(0, 7)));
    assert_eq!(progress.last(), Some(&(3, 7)));
    assert_eq!(saved, vec![2, 1], "saved after every two and at the end");
    assert_eq!(load_cache(&file).len(), 3);
    assert!(!file.with_extension("pending").exists(), "the temporary file of the atomic save is gone");
    // A crash in the middle of a save leaves a temporary file beside the cache, never a torn cache.
    std::fs::write(file.with_extension("pending"), b"{ \"scheme\": 1, \"hashes\": { \"torn").unwrap();
    assert_eq!(load_cache(&file).len(), 3);

    // A second call picks up where the first stopped: only the missing ones are read.
    let mut second = Vec::new();
    let report = backfill(&library.root, &file, &catalog, &cancel, &BackfillOptions::default(), &mut |event| {
        if let Event::Saved { hashes } = event {
            second.extend(hashes.iter().map(|(id, _)| id.clone()));
        }
    })
    .unwrap();
    assert_eq!((report.total, report.hashed), (4, 4));
    assert_eq!(second.len(), 4);
    assert_eq!(load_cache(&file).len(), 7);
    // Nothing left to do: nothing is read, not even if the files are gone.
    std::fs::remove_dir_all(library.root.join("library/models")).unwrap();
    let report = backfill(&library.root, &file, &catalog, &cancel, &BackfillOptions::default(), &mut |_| {}).unwrap();
    assert_eq!((report.total, report.attempted, report.failed.len()), (0, 0, 0));
}

#[test]
fn backfill_skips_items_with_missing_files_and_does_them_once_they_are_back() {
    let base = Base::new("missing");
    let library = library(&base, "a");
    for i in 0..3 {
        import_model(&library, &format!("t{i}"), &format!("uid://m{i}"), &format!("M{i}"), "props", format!("bin{i}").as_bytes(), &[]);
    }
    let file = cache_path(&library);
    forget_hashes(&file);
    let catalog = catalog(&library);
    let broken = find(&catalog, "uid://m1");
    let bin = library.root.join(&broken.fileset().unwrap().bin);
    let saved_bytes = std::fs::read(&bin).unwrap();
    std::fs::remove_file(&bin).unwrap();
    let cancel = AtomicBool::new(false);
    let report = backfill(&library.root, &file, &catalog, &cancel, &BackfillOptions::default(), &mut |_| {}).unwrap();
    assert_eq!((report.hashed, report.failed.len()), (2, 1), "one unreadable item does not fail the run");
    assert_eq!(report.failed, vec![storage_key(broken).unwrap()]);
    // `skip` keeps a session from retrying the same failure.
    let skip: HashSet<String> = report.failed.iter().cloned().collect();
    let again = backfill(&library.root, &file, &catalog, &cancel, &BackfillOptions { skip: &skip, ..Default::default() }, &mut |_| {}).unwrap();
    assert_eq!((again.total, again.attempted), (0, 0));
    std::fs::write(&bin, saved_bytes).unwrap();
    let later = backfill(&library.root, &file, &catalog, &cancel, &BackfillOptions::default(), &mut |_| {}).unwrap();
    assert_eq!((later.total, later.hashed, later.failed.len()), (1, 1, 0));
}

#[test]
fn several_threads_give_the_same_hashes_and_the_same_saves() {
    let base = Base::new("threads");
    let library = library(&base, "a");
    import_model(&library, "s", "uid://shared1", "S1", "props", b"s1", &[("atlas.png", b"atlas")]);
    import_model(&library, "t", "uid://shared2", "S2", "props", b"s2", &[("atlas.png", b"atlas")]);
    for i in 0..20 {
        import_model(&library, &format!("t{i}"), &format!("uid://m{i}"), &format!("M{i}"), "props", format!("bin{i}").as_bytes(), &[("atlas.png", b"atlas")]);
    }
    forget_hashes(&cache_path(&library));
    let catalog = catalog(&library);
    let run = |threads: usize, name: &str| {
        let file = base.0.join(name);
        let mut announced = Vec::new();
        let report = backfill(&library.root, &file, &catalog, &AtomicBool::new(false), &BackfillOptions { threads, save_every: 5, ..Default::default() }, &mut |event| {
            if let Event::Saved { hashes } = event {
                announced.extend(hashes.iter().cloned());
            }
        })
        .unwrap();
        announced.sort();
        (report, announced, load_cache(&file))
    };
    let (one, announced_one, cache_one) = run(1, "one.json");
    let (four, announced_four, cache_four) = run(4, "four.json");
    assert_eq!((one.hashed, four.hashed, four.failed.len()), (22, 22, 0));
    assert_eq!(announced_one, announced_four);
    assert_eq!(announced_four.len(), 22);
    assert_eq!(cache_one.len(), cache_four.len());
    for (id, hash) in &announced_four {
        assert_eq!(hash, &computed(&library, id));
    }
}

#[test]
fn backfill_can_be_cancelled_and_keeps_what_it_finished() {
    let base = Base::new("cancel");
    let library = library(&base, "a");
    for i in 0..4 {
        import_model(&library, &format!("t{i}"), &format!("uid://m{i}"), &format!("M{i}"), "props", format!("bin{i}").as_bytes(), &[]);
    }
    let file = cache_path(&library);
    forget_hashes(&file);
    let catalog = catalog(&library);
    let cancel = AtomicBool::new(false);
    let report = backfill(&library.root, &file, &catalog, &cancel, &BackfillOptions { save_every: 100, ..Default::default() }, &mut |event| {
        if let Event::Progress { done: 2, .. } = event {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();
    assert!(report.cancelled);
    assert_eq!(report.hashed, 2);
    assert_eq!(load_cache(&file).len(), 2, "finished work is saved when the run stops");
    assert!(backfill(&library.root, &file, &catalog, &AtomicBool::new(true), &BackfillOptions::default(), &mut |_| {}).unwrap().cancelled);
}

#[test]
fn a_cache_of_another_scheme_or_a_damaged_one_is_ignored() {
    let base = Base::new("scheme");
    let file = base.0.join("side.db");
    crate::infra::store::hashes::update(&file, |map| { map.insert("models/abcdefabcdefabcd".into(), "aa".into()); }).unwrap();
    assert_eq!(load_cache(&file).get("models/abcdefabcdefabcd").map(String::as_str), Some("aa"));
    stamp_scheme(&file, SCHEME_VERSION + 1);
    assert!(load_cache(&file).is_empty(), "a changed definition starts over");
    fsio::write_bytes(&file, b"{ not json").unwrap();
    assert!(load_cache(&file).is_empty());
    assert!(load_cache(&base.0.join("missing.db")).is_empty());

    // A run after a version bump recomputes everything and writes the running scheme.
    let library = library(&base, "a");
    import_model(&library, "one", "uid://one", "One", "props", b"1", &[]);
    let path = cache_path(&library);
    assert_eq!(scheme_of(&path), SCHEME_VERSION);
    stamp_scheme(&path, SCHEME_VERSION + 7);
    let catalog = catalog(&library);
    assert_eq!(count_unhashed(&catalog), 1, "a cache of another scheme gives no hashes");
    let report = backfill(&library.root, &path, &catalog, &AtomicBool::new(false), &BackfillOptions::default(), &mut |_| {}).unwrap();
    assert_eq!(report.hashed, 1);
    assert_eq!(scheme_of(&path), SCHEME_VERSION);
}

#[test]
fn the_catalog_is_filled_from_the_side_cache_and_nothing_is_stored_in_the_catalog_cache() {
    let base = Base::new("fill");
    let library = library(&base, "a");
    import_model(&library, "one", "uid://one", "One", "props", b"1", &[]);
    let loaded = catalog(&library);
    let hash = find(&loaded, "uid://one").content_hash.clone().expect("hashed by the import that published it");
    assert!(is_hex_sha256(&hash));
    assert_eq!(hash, computed(&library, "uid://one"));
    // The frontend saves what it holds; the stored cache never keeps the fingerprint.
    save_catalog_inner(&library, &loaded).unwrap();
    let stored = serde_json::to_string(&crate::infra::store::Store::for_handle(&library).load_catalog().unwrap().unwrap()).unwrap();
    assert!(!stored.contains("contentHash"), "derived data is not stored in the catalog cache");
    // And it is served from the side cache: it comes back after a reload.
    assert_eq!(find(&catalog(&library), "uid://one").content_hash.as_deref(), Some(hash.as_str()));
    // JSON: camelCase, and absent when unknown.
    let json = serde_json::to_value(find(&loaded, "uid://one")).unwrap();
    assert_eq!(json["contentHash"], hash);
    let mut unknown = loaded.assets[0].clone();
    unknown.content_hash = None;
    assert!(serde_json::to_value(&unknown).unwrap().get("contentHash").is_none());
    assert!(serde_json::from_value::<Asset>(serde_json::to_value(&unknown).unwrap()).unwrap().content_hash.is_none());
}

#[test]
fn an_import_fingerprints_only_what_it_publishes() {
    let base = Base::new("import");
    let library = library(&base, "a");
    import_model(&library, "one", "uid://one", "One", "props", b"1", &[]);
    import_model(&library, "two", "uid://two", "Two", "props", b"2", &[]);
    let file = cache_path(&library);
    // Pretend the library predates fingerprints: an empty cache, three more imports.
    forget_hashes(&file);
    import_model(&library, "three", "uid://three", "Three", "props", b"3", &[]);
    let cache = load_cache(&file);
    assert_eq!(cache.len(), 1, "the new item only; the other two are the backfill's");
    let catalog = catalog(&library);
    assert!(find(&catalog, "uid://three").content_hash.is_some());
    assert!(find(&catalog, "uid://one").content_hash.is_none() && find(&catalog, "uid://two").content_hash.is_none());
    assert_eq!(count_unhashed(&catalog), 2);
}

#[test]
fn an_edit_in_place_is_picked_up_by_rehash_and_gone_items_are_pruned() {
    let base = Base::new("rehash");
    let library = library(&base, "a");
    import_model(&library, "one", "uid://one", "One", "props", b"1", &[]);
    import_model(&library, "two", "uid://two", "Two", "props", b"2", &[]);
    let loaded = catalog(&library);
    let one = find(&loaded, "uid://one");
    let before = one.content_hash.clone().unwrap();
    // Origin alignment rewrites the stored glTF under the same storage key.
    let path = library.root.join(&one.fileset().unwrap().gltf);
    let mut scene: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    scene["nodes"] = json!([{"translation":[0.0, 1.5, 0.0]}]);
    std::fs::write(&path, serde_json::to_vec(&scene).unwrap()).unwrap();
    rehash(&library, &[one]);
    let after = catalog(&library);
    assert_ne!(find(&after, "uid://one").content_hash.as_ref().unwrap(), &before);
    assert_eq!(find(&after, "uid://two").content_hash, find(&loaded, "uid://two").content_hash, "others are not touched");

    // Prune: an entry for an item that is not in the catalog and whose folder is gone is dropped; a fresh folder is kept.
    let file = cache_path(&library);
    super::content_hash::backfill(&library.root, &file, &after, &AtomicBool::new(false), &BackfillOptions::default(), &mut |_| {}).unwrap();
    std::fs::create_dir_all(library.root.join("library/models/00000000000000ee")).unwrap();
    crate::infra::store::hashes::update(&file, |map| {
        map.insert("models/00000000000000ff".into(), "stale".into());
        map.insert("models/00000000000000ee".into(), "just published".into());
    }).unwrap();
    backfill(&library.root, &file, &after, &AtomicBool::new(false), &BackfillOptions { prune: true, ..Default::default() }, &mut |_| {}).unwrap();
    let cache = load_cache(&file);
    assert!(!cache.contains("models/00000000000000ff") && cache.contains("models/00000000000000ee"));
    assert_eq!(cache.len(), 3);
}

#[test]
fn a_transferred_copy_has_the_same_hash_and_is_fingerprinted_on_arrival() {
    use super::transfer::{transfer_between, OnConflict, TransferMode, TransferOptions};
    let base = Base::new("transfer");
    let (a, b) = (library(&base, "a"), library(&base, "b"));
    import_model(&a, "one", "uid://crate", "Crate", "props", b"mesh", &[("wood.png", b"wood")]);
    put(&a, "library/_import_two/sfx/click.wav", &wav(3));
    merge_entries(&a, vec![json!({"id":"uid://click","kind":"audio","pack":"_import_two","category":"sfx","file":"click.wav"})]).unwrap();
    let ids = vec!["uid://crate".to_string(), "uid://click".to_string()];
    let options = TransferOptions { mode: TransferMode::Copy, on_conflict: OnConflict::Skip, include_clips: false };
    transfer_between(&a, &b, &ids, options, &|_| {}).unwrap();
    let (from, to) = (catalog(&a), catalog(&b));
    for id in ["uid://crate", "uid://click"] {
        let hash = find(&to, id).content_hash.clone().unwrap_or_else(|| panic!("{id} was not fingerprinted on arrival"));
        assert_eq!(Some(hash), find(&from, id).content_hash.clone(), "{id}");
    }
}

/// Hashing speed on a synthetic library, for estimating a first run:
/// `cargo test --release -p toybox-core --lib throughput -- --ignored --nocapture`.
/// Files were just written, so this is the CPU-bound figure (a cold disk can be slower).
#[test]
#[ignore = "a measurement, not a check"]
fn throughput_on_a_synthetic_library() {
    use crate::domain::catalog_model::{AnimationMeta, AssetContent, AssetFileset, ThumbMeta, UserMeta};
    let base = Base::new("throughput");
    let root = base.0.join("root");
    let (models, materials, sounds) = (1500usize, 150usize, 300usize);
    let (model_bin, material_map, sound) = (400 * 1024usize, 2 * 1024 * 1024usize, 300 * 1024usize);
    let make = |id: String, rel: String, content: AssetContent| Asset {
        rigs: vec![], variants: vec![], id, name: "n".into(), file_name: "f".into(), rel_path: rel, pack: "p".into(), category: "c".into(), content,
        thumb: ThumbMeta::default(), user: UserMeta::default(), tags: vec![], origin: None, format: None, content_hash: None,
    };
    let mut assets = Vec::new();
    let mut bytes_written = 0usize;
    let filler = |seed: usize, size: usize| -> Vec<u8> { (0..size).map(|i| (i.wrapping_mul(31).wrapping_add(seed)) as u8).collect() };
    // Four shared atlases, as a pack has.
    for atlas in 0..4 {
        let rel = format!("library/models/_resources/{:064x}.png", atlas);
        fsio::write_bytes(&root.join(&rel), &filler(atlas, 4 * 1024 * 1024)).unwrap();
        bytes_written += 4 * 1024 * 1024;
    }
    for i in 0..models {
        let key = format!("{:064x}", i + 1);
        let folder = format!("library/models/{key}");
        let scene = json!({"asset":{"version":"2.0"},"buffers":[{"uri":"m.bin","byteLength":model_bin}],"images":[{"uri":format!("../_resources/{:064x}.png", i % 4)}],"nodes":(0..200).map(|n| json!({"name":format!("node{n}")})).collect::<Vec<_>>()});
        let gltf = serde_json::to_vec_pretty(&scene).unwrap();
        bytes_written += gltf.len() + model_bin;
        fsio::write_bytes(&root.join(format!("{folder}/m.gltf")), &gltf).unwrap();
        fsio::write_bytes(&root.join(format!("{folder}/m.bin")), &filler(i, model_bin)).unwrap();
        assets.push(make(format!("uid://m{i}"), format!("{folder}/m.gltf"), AssetContent::Model {
            fileset: AssetFileset { gltf: format!("{folder}/m.gltf"), bin: format!("{folder}/m.bin"), textures: vec![] }, animation: AnimationMeta::default() }));
    }
    for i in 0..materials {
        let folder = format!("library/materials/{:064x}", 10_000 + i);
        let mut maps = Vec::new();
        for (n, role) in [crate::domain::material::MapRole::Albedo, crate::domain::material::MapRole::Normal, crate::domain::material::MapRole::Roughness, crate::domain::material::MapRole::Ao].into_iter().enumerate() {
            let file = format!("{folder}/map{n}.png");
            fsio::write_bytes(&root.join(&file), &filler(i + n, material_map)).unwrap();
            bytes_written += material_map;
            maps.push(crate::domain::material::MaterialMap { role, file, channel: None, invert: false });
        }
        assets.push(make(format!("uid://mat{i}"), folder, AssetContent::Material { material: crate::domain::material::MaterialSet { maps, flip_normal_green: false } }));
    }
    for i in 0..sounds {
        let file = format!("library/audio/{:064x}/s.wav", 20_000 + i);
        fsio::write_bytes(&root.join(&file), &filler(i, sound)).unwrap();
        bytes_written += sound;
        assets.push(make(format!("uid://snd{i}"), file.clone(), AssetContent::Audio { audio: crate::domain::audio::AudioMeta { file, format: "wav".into(), duration: 1.0, sample_rate: 8000, channels: 1, bit_depth: Some(16), byte_length: 0, modified_ns: String::new(), peaks: vec![] } }));
    }
    // The hash alone, from memory, to tell the CPU from the disk.
    let block = filler(7, 4 * 1024 * 1024);
    let started = std::time::Instant::now();
    let mut sha = Sha256::new();
    for _ in 0..256 {
        sha.update(&block);
    }
    let _ = sha.finalize();
    println!("SHA-256 in memory: {:.0} MB/s", 1024.0 / started.elapsed().as_secs_f64());
    let catalog = Catalog { rigs: vec![], schema_version: 1, library_root: root.to_string_lossy().into(), assets };
    for threads in [4usize, 1, 2, 1] {
        let file = base.0.join(format!("content-hashes-{threads}-{}.json", COUNTER.fetch_add(1, Ordering::SeqCst)));
        let started = std::time::Instant::now();
        let report = backfill(&root, &file, &catalog, &AtomicBool::new(false), &BackfillOptions { threads, ..Default::default() }, &mut |_| {}).unwrap();
        let seconds = started.elapsed().as_secs_f64();
        let items = report.hashed;
        println!("{threads} thread(s): hashed {items} items, {:.0} MB in {seconds:.2} s: {:.0} MB/s, {:.2} ms per item; cache file {} KB",
            bytes_written as f64 / 1e6, bytes_written as f64 / 1e6 / seconds, seconds * 1000.0 / items as f64, std::fs::metadata(&file).unwrap().len() / 1024);
        assert_eq!(items, models + materials + sounds);
    }
    let file = base.0.join("second.json");
    backfill(&root, &file, &catalog, &AtomicBool::new(false), &BackfillOptions { threads: 4, ..Default::default() }, &mut |_| {}).unwrap();
    // The second run reads nothing.
    let again = std::time::Instant::now();
    let report = backfill(&root, &file, &catalog, &AtomicBool::new(false), &BackfillOptions::default(), &mut |_| {}).unwrap();
    println!("second run: {} pending, {:.0} ms", report.total, again.elapsed().as_secs_f64() * 1000.0);
}
