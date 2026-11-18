//! Library storage benchmark: builds a synthetic library of N items through the
//! real import path and times the operations whose cost should not depend on N.
//!
//! ```text
//! bench-library --dir D:\dev\toybox-bench\run --items 20000 [--batch 100] [--label before]
//!               [--report out.json] [--keep] [--reuse] [--lib-name lib] [--generate-only]
//! ```
//!
//! `--generate-only` builds the library and stops (it implies `--keep`): that is how the
//! library for the server benchmark (`toybox-remote/tests/bench_feed.rs`) is made, with
//! `--lib-name library` so that `--dir` can be used as a server data folder.
//!
//! `--query-bench` measures the window's queries (page, deep page, search, tags, counts, assets by id)
//! over two generated libraries of `--items` / 2 rows each, with copies of items across them. The rows
//! are written straight into the databases (`infra::store::bench_data`), not through the import path,
//! so a million items take about a minute to make instead of two hours. `--reps N` repeats each query.
//!
//! `--final --dir D --sizes 100000,1000000 [--sync-json 100000=a.json,1000000=b.json] [--table-out t.md]` runs the
//! query benchmark and the write operations for each size as child processes (two libraries of N/2 rows for the
//! queries, one of N rows for the writes, both written directly) and prints one table with the medians and the
//! figures of before step 5; the sync figures come from `bench_sync.rs`'s output files.
//!
//! `--table-only` prints the table from the results of an earlier `--final` (the json files in `--dir`).
//!
//! `--direct` writes the library of `--items` rows straight into its database (items and derived rows, no files)
//! instead of through the import path, which cannot reach a million items: used for the wire benchmarks (a server
//! data folder, with `--lib-name library --generate-only`) and for the write operations below at 100,000 and 1,000,000.
//!
//! `--reuse` skips generation and runs the operations on the library already in
//! `--dir` (the item count must be the same N it was generated with).
//! Everything is written below `--dir` (library, app data, a small second
//! library for the transfer); it is deleted at the end unless `--keep` is given.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{json, Value};
use toybox_core::domain::catalog_model::ThumbState;
use toybox_core::infra::registry::{LibraryEntry, LibraryHandle};
use toybox_core::ops::metadata::MetadataEdit;
use toybox_core::ops::transfer::{self, ExportOptions, IngestOptions};

struct Args {
    dir: PathBuf,
    items: usize,
    batch: usize,
    label: String,
    report: Option<PathBuf>,
    keep: bool,
    reuse: bool,
    lib_name: String,
    generate_only: bool,
    query_bench: bool,
    reps: usize,
    no_index: bool,
    direct: bool,
    table_only: bool,
    final_run: bool,
    sizes: Vec<usize>,
    sync_json: Vec<(usize, PathBuf)>,
    table_out: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = Args { dir: PathBuf::new(), items: 2000, batch: 100, label: "run".into(), report: None, keep: false, reuse: false, lib_name: "lib".into(), generate_only: false, query_bench: false, reps: 5, no_index: false, direct: false, table_only: false, final_run: false, sizes: vec![100_000, 1_000_000], sync_json: Vec::new(), table_out: None };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--dir" => args.dir = PathBuf::from(it.next().expect("--dir needs a value")),
            "--items" => args.items = it.next().expect("--items needs a value").parse().expect("a number"),
            "--batch" => args.batch = it.next().expect("--batch needs a value").parse().expect("a number"),
            "--label" => args.label = it.next().expect("--label needs a value"),
            "--report" => args.report = Some(PathBuf::from(it.next().expect("--report needs a value"))),
            "--keep" => args.keep = true,
            "--reuse" => args.reuse = true,
            "--lib-name" => args.lib_name = it.next().expect("--lib-name needs a value"),
            "--generate-only" => args.generate_only = true,
            "--query-bench" => args.query_bench = true,
            "--no-index" => args.no_index = true,
            "--direct" => args.direct = true,
            "--final" => args.final_run = true,
            "--table-only" => args.table_only = true,
            "--sizes" => args.sizes = it.next().expect("--sizes needs a value").split(',').map(|n| n.trim().parse().expect("a number")).collect(),
            "--sync-json" => {
                // size=path[,size=path]: the results of `toybox-remote/tests/bench_sync.rs` (it writes them when TOYBOX_BENCH_OUT is set).
                for pair in it.next().expect("--sync-json needs a value").split(',') {
                    let (size, path) = pair.split_once('=').expect("size=path");
                    args.sync_json.push((size.parse().expect("a number"), PathBuf::from(path)));
                }
            }
            "--table-out" => args.table_out = Some(PathBuf::from(it.next().expect("--table-out needs a value"))),
            "--reps" => args.reps = it.next().expect("--reps needs a value").parse().expect("a number"),
            other => panic!("unknown argument {other}"),
        }
    }
    assert!(!args.dir.as_os_str().is_empty(), "--dir is required");
    assert!(args.dir.starts_with("D:\\dev\\toybox-bench") || args.dir.starts_with("D:/dev/toybox-bench") || args.dir.starts_with(std::env::temp_dir()), "the benchmark only runs below D:\\dev\\toybox-bench or the temp folder");
    args
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn wav(n: usize) -> Vec<u8> {
    let samples: Vec<i16> = (0..128).map(|i| (((i * (n % 50 + 1)) % 200) as i16 - 100) * 100).collect();
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8000u32.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

fn png(n: usize) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([(n % 251) as u8, ((n / 251) % 251) as u8, ((n / 63001) % 251) as u8, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// Stage item `n` the way an importer would (files below `library/<pack>/...`)
/// and return its entry. `prefix` keeps ids of different libraries apart.
fn stage(root: &Path, prefix: &str, n: usize, pack: &str) -> Value {
    let id = format!("uid://{prefix}{n}");
    let provenance = json!({"source": format!("Pack {}", n % 200), "publisher": format!("Publisher {}", n % 50), "license": "CC0", "filesystemPath": format!("P:/bench/{n}")});
    let tags = json!([format!("tag{}", n % 97), format!("group{}", n % 1000), format!("kind{}", n % 20)]);
    match n % 20 {
        0..=13 => {
            let name = format!("M{n}");
            let mut scene = json!({
                "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [0]}],
                "nodes": [{"name": "Root", "children": [1]}, {"name": format!("Part{}", n % 16), "mesh": 0}],
                "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
                "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]}],
                "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}],
                "buffers": [{"uri": format!("{name}.bin"), "byteLength": 36}]
            });
            if n % 8 == 0 {
                let texture = n % 64;
                scene["images"] = json!([{"uri": format!("../textures/Tex{texture}.png")}]);
                scene["textures"] = json!([{"source": 0}]);
                put(root, &format!("library/{pack}/textures/Tex{texture}.png"), &png(texture));
            }
            put(root, &format!("library/{pack}/props/{name}.gltf"), &serde_json::to_vec(&scene).unwrap());
            let mut bin = vec![0u8; 36];
            bin[..8].copy_from_slice(&(n as u64).to_le_bytes());
            put(root, &format!("library/{pack}/props/{name}.bin"), &bin);
            json!({"id": id, "pack": pack, "category": "props", "file": format!("{name}.gltf"), "format": "fbx", "tags": tags, "provenance": provenance})
        }
        14..=16 => {
            let file = format!("A{n}.wav");
            put(root, &format!("library/{pack}/sfx/{file}"), &wav(n));
            json!({"id": id, "kind": "audio", "pack": pack, "category": "sfx", "file": file, "format": "wav", "tags": tags, "provenance": provenance})
        }
        17 | 18 => {
            let name = format!("Mat{n}");
            put(root, &format!("library/{pack}/materials/{name}/albedo.png"), &png(n));
            json!({"id": id, "kind": "material", "pack": pack, "category": "stone", "name": name, "maps": [{"role": "albedo", "file": "albedo.png"}], "format": "png", "tags": tags, "provenance": provenance})
        }
        _ => {
            let name = format!("Decal{n}");
            put(root, &format!("library/{pack}/decal/{name}/a.png"), &png(n + 7));
            json!({"id": id, "kind": "plugin", "type": "decal", "pack": pack, "category": "decals", "name": name, "files": ["a.png"], "meta": {}, "format": "png", "tags": tags, "provenance": provenance})
        }
    }
}

fn make_library(base: &Path, name: &str) -> LibraryHandle {
    let root = base.join(name);
    toybox_core::infra::library::ensure_library_folder(&root).unwrap();
    let root = root.canonicalize().unwrap();
    LibraryHandle::local(&base.join("appdata"), &LibraryEntry::local(name, name, &root.to_string_lossy()))
}

fn time<T>(label: &str, f: impl FnOnce() -> T) -> (T, f64) {
    let start = Instant::now();
    let value = f();
    let secs = start.elapsed().as_secs_f64();
    eprintln!("{label}: {secs:.3}s");
    (value, secs)
}

fn spread(n: usize, count: usize, offset: usize) -> Vec<usize> {
    let step = (n / count).max(1);
    (0..count).map(|i| (i * step + offset) % n).collect()
}

fn dir_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn main() {
    let args = parse_args();
    if args.final_run {
        final_run(&args);
        return;
    }
    if args.query_bench {
        query_bench(&args);
        return;
    }
    let n = args.items;
    std::fs::create_dir_all(&args.dir).unwrap();
    let library = if args.reuse {
        let root = args.dir.join(&args.lib_name).canonicalize().expect("--reuse needs a generated library");
        LibraryHandle::local(&args.dir.join("appdata"), &LibraryEntry::local(&args.lib_name, &args.lib_name, &root.to_string_lossy()))
    } else {
        make_library(&args.dir, &args.lib_name)
    };
    let mut report = json!({"label": args.label, "items": n});

    // ---- generation ---------------------------------------------------------------------
    if !args.reuse && args.direct {
        // Rows written straight into the databases (items and derived rows, no files): what the import path would take hours to make.
        use toybox_core::infra::store::bench_data::{generate, Spec};
        std::fs::create_dir_all(library.user_db_path().parent().unwrap()).unwrap();
        // The library is at the latest migration already (the migration steps read every item's files).
        std::fs::write(library.root.join("_library_config").join("migrations.json"), json!({"version": toybox_core::domain::library_migrations::LATEST_VERSION, "applied": []}).to_string()).unwrap();
        let t = Instant::now();
        generate(&library.library_db_path(), &library.user_db_path(), &Spec { prefix: "b", count: n, tag_vocabulary: 3000, copy_of: None, same_id_every: 0, unhashed_every: 0 }).unwrap();
        eprintln!("generated {n} rows directly in {:.1}s", t.elapsed().as_secs_f64());
        report["generation"] = json!({"direct": true, "totalSecs": t.elapsed().as_secs_f64()});
    } else if !args.reuse {
        let start = Instant::now();
        let mut done = 0usize;
        let mut batch_no = 0usize;
        let mut stage_secs = 0.0;
        let mut merge_secs = 0.0;
        let mut first_merge = None;
        let mut last_merge = 0.0;
        while done < n {
            let count = args.batch.min(n - done);
            let pack = format!("_import_g{batch_no}");
            let staged = Instant::now();
            let entries: Vec<Value> = (done..done + count).map(|i| stage(&library.root, "b", i, &pack)).collect();
            stage_secs += staged.elapsed().as_secs_f64();
            let merged = Instant::now();
            toybox_core::ops::import::merge_entries(&library, entries).unwrap_or_else(|e| panic!("import batch {batch_no} failed: {e}"));
            last_merge = merged.elapsed().as_secs_f64();
            merge_secs += last_merge;
            first_merge.get_or_insert(last_merge);
            done += count;
            batch_no += 1;
            if batch_no % 10 == 0 || done == n {
                eprintln!("generated {done}/{n} items, {:.0}s elapsed, last batch merge {last_merge:.2}s", start.elapsed().as_secs_f64());
            }
        }
        let total = start.elapsed().as_secs_f64();
        report["generation"] = json!({
            "batch": args.batch, "totalSecs": total, "itemsPerSec": n as f64 / total,
            "stagingSecs": stage_secs, "mergeSecs": merge_secs,
            "firstBatchMergeSecs": first_merge, "lastBatchMergeSecs": last_merge,
        });
    }

    if args.generate_only {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return;
    }

    // ---- operations ------------------------------------------------------------
    let mut ops = serde_json::Map::new();
    let mut record = |name: &str, secs: f64, extra: Value| {
        ops.insert(name.into(), json!({"secs": secs, "extra": extra}));
    };

    // Which rows the operations touch: in a directly generated library only audio rows have manifest entries, and the files of
    // the ones that are edited or exported are made here (the generator writes rows, not files).
    let direct = args.direct;
    let pick = |i: usize| -> usize { if direct && i % 5 == 4 { i - 1 } else { i } };
    let idof = |i: usize| -> String { if direct { format!("b{i}") } else { format!("uid://b{i}") } };
    if direct {
        for i in spread(n, 100, 1).into_iter().chain(spread(n, 100, 5)).chain(spread(n, 100, 3)).map(pick) {
            put(&library.root, &toybox_core::infra::store::bench_data::item_file(i), &wav(i));
        }
    }

    // (a)-(d), (f), (j): repeated on different items each time (`--reps`, at most 5); the table shows the median.
    let mut samples: std::collections::BTreeMap<&str, Vec<f64>> = std::collections::BTreeMap::new();
    let mut copied = (0, 0);
    let source = make_library(&args.dir, "source");
    for rep in 0..args.reps.clamp(1, 5) {
        // (a) import 100 new items.
        let pack = format!("_import_new{rep}");
        let first = n + rep * 1000;
        let entries: Vec<Value> = (first..first + 100).map(|i| stage(&library.root, "b", i, &pack)).collect();
        let (_, secs) = time("a_import_100", || toybox_core::ops::import::merge_entries(&library, entries).unwrap());
        samples.entry("a_import_100").or_default().push(secs);

        // (b) edit tags on 100 items.
        let edits: Vec<MetadataEdit> = spread(n, 100, 1 + rep * 7).into_iter().map(pick).map(|i| MetadataEdit { id: idof(i), tags: Some(vec![format!("bench-edit{rep}"), format!("e{}", i % 7)]), provenance: None }).collect();
        let (_, secs) = time("b_edit_tags_100", || toybox_core::ops::metadata::update_entries(&library, &edits).unwrap());
        samples.entry("b_edit_tags_100").or_default().push(secs);

        // (c) delete 100 items.
        let doomed: Vec<String> = spread(n, 100, 3 + rep * 11).into_iter().map(pick).map(idof).collect();
        let (_, secs) = time("c_delete_100", || toybox_core::ops::delete::delete_from_library(&library.root, &library.cache_path, &doomed).unwrap());
        samples.entry("c_delete_100").or_default().push(secs);

        // (d) ingest a 100-item transfer package from a second small library.
        let prefix = format!("s{rep}x");
        let pack = format!("_import_s{rep}");
        let entries: Vec<Value> = (0..100).map(|i| stage(&source.root, &prefix, i, &pack)).collect();
        toybox_core::ops::import::merge_entries(&source, entries).unwrap();
        let ids: Vec<String> = (0..100).map(|i| format!("uid://{prefix}{i}")).collect();
        let package = transfer::export_package(&source, &ids, ExportOptions::default()).unwrap();
        assert!(package.failed.is_empty(), "package failures: {:?}", package.failed);
        let reader = transfer::local_reader(&source);
        let (transferred, secs) = time("d_ingest_100", || transfer::ingest_package(&library, &package, IngestOptions::default(), &reader, &|_| {}).unwrap());
        samples.entry("d_ingest_100").or_default().push(secs);
        copied = (transferred.count(transfer::Outcome::Copied), transferred.count(transfer::Outcome::Failed));

        // (f) set one thumbnail state.
        let (_, secs) = time("f_set_thumb_state", || toybox_core::ops::thumbs::set_thumb_state(&library, &idof(pick(n / 2 + 2 + rep * 5)), ThumbState::Error, Some("bench".into())).unwrap());
        samples.entry("f_set_thumb_state").or_default().push(secs);

        // (j) set one favourite (user.db only).
        let favourite = idof(pick(n / 3 + rep * 5));
        let (_, secs) = time("j_set_favorite", || toybox_core::infra::store::Store::for_handle(&library).set_favorite(std::slice::from_ref(&favourite), true).unwrap());
        samples.entry("j_set_favorite").or_default().push(secs);
    }
    for (name, mut values) in samples {
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let extra = if name == "d_ingest_100" { json!({"copied": copied.0, "failed": copied.1, "samples": values.clone()}) } else { json!({"samples": values.clone()}) };
        record(name, values[values.len() / 2], extra);
    }

    // (e) load the whole catalog (not done past 200,000: it is O(N) by nature, and no longer on any path of the window).
    let catalog = if n <= 200_000 {
        let (catalog, secs) = time("e_load_catalog", || toybox_core::ops::catalog::load_catalog_inner(&library).unwrap().unwrap());
        record("e_load_catalog", secs, json!({"assets": catalog.assets.len(), "rigs": catalog.rigs.len()}));
        Some(catalog)
    } else {
        None
    };

    // (g) list pending thumbnails.
    let (pending, secs) = time("g_list_pending_thumbs", || toybox_core::ops::thumbs::list_pending_thumbs(&library).unwrap());
    record("g_list_pending_thumbs", secs, json!({"pending": pending.len()}));

    // (h) export a package of 100 ids.
    let wanted: Vec<String> = spread(n, 100, 5).into_iter().map(pick).map(idof).collect();
    let (exported, secs) = time("h_export_package_100", || transfer::export_package(&library, &wanted, ExportOptions::default()).unwrap());
    record("h_export_package_100", secs, json!({"assets": exported.assets.len(), "failed": exported.failed.len()}));

    // (i) content-hash 100 new items (the ones imported in (a), their hashes forgotten first).
    let fresh_ids: Vec<String> = (n + 4000..n + 4100).map(|i| format!("uid://b{i}")).collect();
    let (secs, hashed) = hash_new_bench(&library, &fresh_ids);
    eprintln!("i_hash_new_100: {secs:.3}s");
    record("i_hash_new_100", secs, json!({"hashed": hashed}));

    report["ops"] = Value::Object(ops);
    report["libraryDbBytes"] = json!(dir_size(&library.library_db_path()));
    report["userDbBytes"] = json!(dir_size(&library.user_db_path()));
    let text = serde_json::to_string_pretty(&report).unwrap();
    println!("{text}");
    if let Some(path) = &args.report {
        std::fs::write(path, &text).unwrap();
    }
    if !args.keep {
        drop(catalog);
        let _ = std::fs::remove_dir_all(&args.dir);
    }
}

/// Forget the hashes of `ids`, then time `hash_new` for them.
fn hash_new_bench(library: &LibraryHandle, ids: &[String]) -> (f64, usize) {
    let store = toybox_core::infra::store::Store::for_handle(library);
    let mut assets = store.load_assets(ids).unwrap();
    let mut keys = Vec::new();
    for asset in &assets {
        keys.extend(toybox_core::ops::content_hash::storage_key(asset));
    }
    toybox_core::infra::store::hashes::update_keys(&library.library_db_path(), &[], &keys).unwrap();
    let start = Instant::now();
    let hashed = toybox_core::ops::content_hash::hash_new(library, &mut assets);
    (start.elapsed().as_secs_f64(), hashed)
}

// ---- the window's queries ---------------------------------------------------------

fn query_bench(args: &Args) {
    use toybox_core::infra::store::bench_data::{generate, Spec};
    use toybox_core::infra::store::browse::{BrowseFilter, Visibility};
    use toybox_core::ops::browse::{self, BrowseLibrary, ItemRef};

    let n = args.items;
    let each = n / 2;
    std::fs::create_dir_all(&args.dir).unwrap();
    let mut libraries = Vec::new();
    for (index, name) in ["qa", "qb"].iter().enumerate() {
        let handle = if args.reuse {
            let root = args.dir.join(name).canonicalize().expect("--reuse needs generated libraries");
            LibraryHandle::local(&args.dir.join("appdata"), &LibraryEntry::local(name, name, &root.to_string_lossy()))
        } else {
            make_library(&args.dir, name)
        };
        libraries.push(BrowseLibrary { handle, rank: index as i64 });
    }
    if !args.reuse {
        let start = Instant::now();
        for (index, library) in libraries.iter().enumerate() {
            let spec = Spec {
                prefix: if index == 0 { "a" } else { "b" },
                count: each,
                tag_vocabulary: 3000,
                copy_of: if index == 0 { None } else { Some(("a", each, 10)) },
                same_id_every: if index == 0 { 0 } else { 50 },
                unhashed_every: 500,
            };
            std::fs::create_dir_all(library.handle.user_db_path().parent().unwrap()).unwrap();
            generate(&library.handle.library_db_path(), &library.handle.user_db_path(), &spec).unwrap();
        }
        eprintln!("generated {n} rows in {:.1}s", start.elapsed().as_secs_f64());
    }
    let mut index_secs = 0.0;
    let mut report = json!({"items": n, "librariesOf": each, "dbBytes": [dir_size(&libraries[0].handle.library_db_path()), dir_size(&libraries[1].handle.library_db_path())]});
    let reps = args.reps.max(1);
    let mut measure = |label: &str, mut run: Box<dyn FnMut() -> Value + '_>| {
        let mut times = Vec::new();
        let mut detail = Value::Null;
        for _ in 0..reps {
            let start = Instant::now();
            detail = run();
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        let first = times[0];
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = times[times.len() / 2];
        eprintln!("{label:<48} {median:>9.1} ms (first {first:.1}, max {:.1})  {detail}", times[times.len() - 1]);
        report[label] = json!({"medianMs": median, "firstMs": first, "maxMs": times[times.len() - 1], "detail": detail});
    };
    let filter = |edit: &dyn Fn(&mut BrowseFilter)| {
        let mut f = BrowseFilter { kind: "audio".into(), ..Default::default() };
        edit(&mut f);
        f
    };
    let none = Visibility::default();
    let query = |f: BrowseFilter, offset: i64| {
        let libraries = libraries.clone();
        Box::new(move || {
            let page = browse::query_assets(&libraries, &f, false, offset, 100).unwrap();
            json!({"total": page.total, "exact": page.exact, "rows": page.rows.len()})
        }) as Box<dyn FnMut() -> Value>
    };
    {
        use toybox_core::infra::store::browse::Session;
        let sources: Vec<toybox_core::infra::store::browse::Source> = libraries.iter().map(|l| toybox_core::infra::store::browse::Source { id: l.handle.id.clone(), library: l.handle.library_db_path(), user: l.handle.user_db_path(), rank: l.rank }).collect();
        for (label, f) in [("all", filter(&|_| {})), ("text stone", filter(&|f| f.text = "stone".into())), ("source", filter(&|f| f.sources = vec!["Pack 7".into()]))] {
            let t = Instant::now();
            let session = Session::open(&sources, &[]).unwrap();
            let open = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let _ = session.page(&f, false, 0, 100).unwrap();
            let page = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let n = session.count(&f).unwrap();
            let count = t.elapsed().as_secs_f64() * 1000.0;
            eprintln!("profile {label}: open {open:.1} ms, page {page:.1} ms, count {count:.1} ms ({n})");
            if std::env::var("PLAN").is_ok() {
                for line in session.explain_page(&f, 0, 100).unwrap() { eprintln!("   {line}"); }
            }
        }
    }
    if !args.no_index {
        let t = Instant::now();
        browse::warm_index(&libraries).unwrap();
        eprintln!("browse index built/caught up in {:.2}s", t.elapsed().as_secs_f64());
        index_secs = t.elapsed().as_secs_f64();
    } else {
        // Leave the index out: a file that is not there is not used, and nothing builds one.
        let _ = std::fs::remove_file(args.dir.join("appdata").join("browse-index.db"));
    }
    if std::env::var("PROFILE_OPEN").is_ok() {
        let path = libraries[0].handle.library_db_path();
        let t = Instant::now();
        let conn = rusqlite::Connection::open_in_memory_with_flags(rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_URI).unwrap();
        eprintln!("open mem {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        conn.execute("ATTACH DATABASE ?1 AS l0", [format!("file:/{}?mode=ro", path.to_string_lossy().replace('\\', "/"))]).unwrap();
        eprintln!("attach {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let _: String = conn.query_row("SELECT value FROM l0.meta WHERE key='revision'", [], |r| r.get(0)).unwrap();
        eprintln!("first query {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let _: Option<i64> = conn.query_row("SELECT 1 FROM l0.assets WHERE content_hash IS NULL LIMIT 1", [], |r| r.get(0)).ok();
        eprintln!("unhashed probe {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let _: Option<i64> = conn.query_row("SELECT 1 FROM l0.assets LIMIT 1", [], |r| r.get(0)).ok();
        eprintln!("assets probe {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    }
    let total = browse::count_assets(&libraries, &filter(&|_| {})).unwrap();
    eprintln!("items shown for audio: {total}");
    {
        let libraries = libraries.clone();
        let none = none.clone();
        measure("open/start (summaries + type counts)", Box::new(move || {
            let counts: i64 = libraries.iter().map(|l| browse::summary(&l.handle).unwrap().count).sum();
            json!({"items": counts, "types": browse::type_counts(&libraries, &none).unwrap().len()})
        }));
    }
    measure("first page (100 rows + total)", query(filter(&|_| {}), 0));
    measure("deep page (middle of the list)", query(filter(&|_| {}), total / 2));
    measure("deepest page", query(filter(&|_| {}), total - 100));
    measure("text, 2 characters (ro)", query(filter(&|f| f.text = "ro".into()), 0));
    measure("text, 5 characters (stone)", query(filter(&|f| f.text = "stone".into()), 0));
    measure("text, 5+ characters, rare (tower 976)", query(filter(&|f| f.text = "tower 976".into()), 0));
    measure("text, 5 characters, 1% of items (tag29)", query(filter(&|f| f.text = "tag29".into()), 0));
    measure("tag filter, common (stone)", query(filter(&|f| f.tags = vec!["stone".into()]), 0));
    measure("tag filter, rare (tag1500)", query(filter(&|f| f.tags = vec!["tag1500".into()]), 0));
    measure("category filter", query(filter(&|f| f.categories = vec!["sfx".into()]), 0));
    measure("source filter", query(filter(&|f| f.sources = vec!["Pack 7".into()]), 0));
    measure("favourites", query(filter(&|f| f.favorites_only = true), 0));
    measure("collection", query(filter(&|f| f.collection = Some("Collection 3".into())), 0));
    measure("category + tag + text", query(filter(&|f| { f.categories = vec!["props".into()]; f.tags = vec!["wood".into()]; f.text = "crate".into(); }), 0));
    measure("only library qb", query(filter(&|f| f.libraries = vec!["qb".into()]), 0));
    {
        let libraries = libraries.clone();
        measure("select all ids (cap 10000)", Box::new(move || {
            let page = browse::query_ids(&libraries, &BrowseFilter { kind: "audio".into(), ..Default::default() }, false, 0, 10_000).unwrap();
            json!({"ids": page.ids.len(), "truncated": page.truncated})
        }));
    }
    {
        // After a change: one asset of the first library is renamed (a revision, a row in `changes`), as any writer does.
        let libraries = libraries.clone();
        let none = none.clone();
        let db = libraries[0].handle.library_db_path();
        let mut counter = 0u64;
        measure("facet counts, first after a change", Box::new(move || {
            counter += 1;
            toybox_core::infra::store::bench_data::touch_many(&db, &[format!("a{}", 1000 + counter)], &format!("edit{counter}")).unwrap();
            let facets = browse::facet_counts(&libraries, "audio", &none, 60, &[]).unwrap();
            json!({"total": facets.total, "tags": facets.tags.len(), "tagTotal": facets.tag_total})
        }));
    }
    {
        // With a plugin switched off the duplicate rows are found by a pass over each library (the index is not used).
        let libraries = libraries.clone();
        let none = none.clone();
        measure("facet counts, first with a plugin hidden", Box::new(move || {
            let mut visibility = none.clone();
            visibility.hidden_types = vec![format!("nothing{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())];
            let facets = browse::facet_counts(&libraries, "audio", &visibility, 60, &[]).unwrap();
            json!({"total": facets.total})
        }));
    }
    {
        let libraries = libraries.clone();
        let none = none.clone();
        measure("facet counts, remembered", Box::new(move || {
            let facets = browse::facet_counts(&libraries, "audio", &none, 60, &["stone".to_string()]).unwrap();
            json!({"total": facets.total, "categories": facets.categories.len()})
        }));
    }
    {
        let libraries = libraries.clone();
        let none = none.clone();
        measure("tag search (remembered)", Box::new(move || json!({"found": browse::search_tags(&libraries, "audio", &none, "tag1", 60).unwrap().len()})));
    }
    {
        let libraries = libraries.clone();
        let none = none.clone();
        measure("type counts", Box::new(move || json!({"types": browse::type_counts(&libraries, &none).unwrap().len()})));
    }
    {
        let libraries = libraries.clone();
        let none = none.clone();
        measure("collections", Box::new(move || json!({"collections": browse::collections(&libraries, "audio", &none).unwrap().len()})));
    }
    {
        let libraries = libraries.clone();
        let items: Vec<ItemRef> = (0..100).map(|i| ItemRef { library: "qa".into(), id: format!("a{}", i * (each / 100)) }).collect();
        measure("get 100 assets by id", Box::new(move || json!({"rows": browse::get_assets(&libraries, &items).unwrap().len()})));
    }
    {
        let libraries = libraries.clone();
        let none = none.clone();
        let items: Vec<ItemRef> = (0..100).map(|i| ItemRef { library: "qb".into(), id: format!("b{}", i * (each / 100)) }).collect();
        measure("where else (100 items)", Box::new(move || json!({"found": browse::elsewhere(&libraries, &items, &none).unwrap().len()})));
    }
    report["indexBuildSecs"] = json!(index_secs);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if let Some(path) = &args.report {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
    if !args.keep {
        let _ = std::fs::remove_dir_all(&args.dir);
    }
}


// ---- the final table ----------------------------------------------------------------

/// Where each row of the final table comes from. `before` is (100,000, 1,000,000) as recorded in docs/sqlite-storage.md before
/// step 5 (`None`: not measured then).
struct Row {
    name: &'static str,
    key: &'static str,
    before: (Option<f64>, Option<f64>),
}

const ROWS: &[Row] = &[
    Row { name: "open / start (summaries + type counts)", key: "open/start (summaries + type counts)", before: (None, None) },
    Row { name: "first page (100 rows + total)", key: "first page (100 rows + total)", before: (Some(20.0), Some(58.0)) },
    Row { name: "middle page", key: "deep page (middle of the list)", before: (Some(45.0), Some(423.0)) },
    Row { name: "last page", key: "deepest page", before: (Some(71.0), Some(772.0)) },
    Row { name: "text, 2 characters (page; total follows)", key: "text, 2 characters (ro)", before: (Some(47.0), Some(411.0)) },
    Row { name: "text, rare (page + total)", key: "text, 5+ characters, rare (tower 976)", before: (None, None) },
    Row { name: "text, 5 characters, 1% of items (page)", key: "text, 5 characters, 1% of items (tag29)", before: (Some(69.0), Some(582.0)) },
    Row { name: "text, 5 characters, broad (page)", key: "text, 5 characters (stone)", before: (Some(75.0), Some(728.0)) },
    Row { name: "tag, common", key: "tag filter, common (stone)", before: (Some(31.0), Some(63.0)) },
    Row { name: "tag, rare", key: "tag filter, rare (tag1500)", before: (Some(19.0), Some(70.0)) },
    Row { name: "category", key: "category filter", before: (Some(20.0), Some(59.0)) },
    Row { name: "favourites", key: "favourites", before: (Some(24.0), Some(72.0)) },
    Row { name: "collection", key: "collection", before: (Some(23.0), Some(66.0)) },
    Row { name: "category + tag + text", key: "category + tag + text", before: (Some(57.0), Some(326.0)) },
    Row { name: "facet counts, warm", key: "facet counts, remembered", before: (Some(12.0), Some(52.0)) },
    Row { name: "facet counts, first after a change", key: "facet counts, first after a change", before: (Some(132.0), Some(1581.0)) },
    Row { name: "facet counts, first with a plugin hidden", key: "facet counts, first with a plugin hidden", before: (Some(132.0), Some(1581.0)) },
    Row { name: "type counts", key: "type counts", before: (Some(8.0), Some(47.0)) },
    Row { name: "get 100 assets by id", key: "get 100 assets by id", before: (Some(8.0), Some(9.0)) },
    Row { name: "select all ids (10,000)", key: "select all ids (cap 10000)", before: (Some(16.0), Some(53.0)) },
];

const WRITES: &[(&str, &str, Option<f64>)] = &[
    ("import 100", "a_import_100", Some(0.74)),
    ("edit tags of 100", "b_edit_tags_100", Some(0.28)),
    ("delete 100", "c_delete_100", Some(0.45)),
    ("ingest a 100-item package", "d_ingest_100", Some(1.18)),
    ("set a favourite", "j_set_favorite", None),
    ("set one thumbnail state", "f_set_thumb_state", Some(0.007)),
];

fn final_run(args: &Args) {
    let exe = std::env::current_exe().unwrap();
    std::fs::create_dir_all(&args.dir).unwrap();
    let mut queries: Vec<(usize, Value)> = Vec::new();
    let mut writes: Vec<(usize, Value)> = Vec::new();
    for &n in &args.sizes {
        let (q, w) = (args.dir.join(format!("q{n}")), args.dir.join(format!("w{n}")));
        let (qr, wr) = (args.dir.join(format!("q{n}.json")), args.dir.join(format!("w{n}.json")));
        let run = |extra: &[&str]| {
            let status = std::process::Command::new(&exe).args(extra).status().expect("run bench-library");
            assert!(status.success(), "bench-library {extra:?} failed");
        };
        if args.table_only {
            queries.push((n, serde_json::from_slice(&std::fs::read(&qr).unwrap()).unwrap()));
            writes.push((n, serde_json::from_slice(&std::fs::read(&wr).unwrap()).unwrap()));
            continue;
        }
        eprintln!("== {n} items: queries");
        let (n_text, q_text, qr_text, reps_text) = (n.to_string(), q.to_string_lossy().to_string(), qr.to_string_lossy().to_string(), args.reps.to_string());
        let mut query_args = vec!["--query-bench", "--items", &n_text, "--dir", &q_text, "--reps", &reps_text, "--report", &qr_text];
        if args.reuse {
            // Libraries made by an earlier run are measured again and left in place.
            query_args.extend(["--reuse", "--keep"]);
        }
        run(&query_args);
        eprintln!("== {n} items: writes");
        let (w_text, wr_text) = (w.to_string_lossy().to_string(), wr.to_string_lossy().to_string());
        run(&["--direct", "--items", &n_text, "--dir", &w_text, "--reps", &reps_text, "--report", &wr_text]);
        queries.push((n, serde_json::from_slice(&std::fs::read(&qr).unwrap()).unwrap()));
        writes.push((n, serde_json::from_slice(&std::fs::read(&wr).unwrap()).unwrap()));
    }
    let sync: Vec<(usize, Value)> = args.sync_json.iter().map(|(n, path)| (*n, serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())).collect();
    let table = final_table(&args.sizes, &queries, &writes, &sync);
    println!("{table}");
    if let Some(path) = &args.table_out {
        std::fs::write(path, &table).unwrap();
    }
}

fn final_table(sizes: &[usize], queries: &[(usize, Value)], writes: &[(usize, Value)], sync: &[(usize, Value)]) -> String {
    let find = |set: &[(usize, Value)], n: usize| set.iter().find(|(size, _)| *size == n).map(|(_, v)| v.clone());
    let fmt = |value: Option<f64>, decimals: usize| value.map(|v| format!("{v:.decimals$}")).unwrap_or_else(|| "-".into());
    let (a, b) = (sizes[0], sizes.get(1).copied().unwrap_or(0));
    let mut out = String::new();
    out.push_str(&format!("Reads (ms, median of runs; each includes opening the connection, the page, the total when cheap and loading the 100 assets)\n\n| query | before {a} | after {a} | before {b} | after {b} |\n|---|---|---|---|---|\n"));
    for row in ROWS {
        let cell = |n: usize| find(queries, n).and_then(|v| v[row.key]["medianMs"].as_f64());
        let mark = |value: Option<f64>, limit: f64| value.map(|v| if v > limit { format!("**{v:.1}**") } else { format!("{v:.1}") }).unwrap_or_else(|| "-".into());
        out.push_str(&format!("| {} | {} | {} | {} | {} |\n", row.name, fmt(row.before.0, 0), mark(cell(a), 50.0), fmt(row.before.1, 0), mark(cell(b), 150.0)));
    }
    out.push_str(&format!("\nWrites (seconds, 100 items per operation unless said)\n\n| operation | before {a} | after {a} | after {b} |\n|---|---|---|---|\n"));
    for (name, key, before) in WRITES {
        let cell = |n: usize| find(writes, n).and_then(|v| v["ops"][key]["secs"].as_f64());
        out.push_str(&format!("| {name} | {} | {} | {} |\n", fmt(*before, 3), fmt(cell(a), 3), fmt(cell(b), 3)));
    }
    out.push_str(&format!("\nSync (loopback server, library written directly)\n\n| | after {a} | after {b} |\n|---|---|---|\n"));
    for (name, key, decimals) in [
        ("server: first page of the feed from 0 (s)", "serverFirstPageSecs", 3),
        ("server: a page 80% into the feed (s)", "serverDeepPageSecs", 3),
        ("changes feed for 100 rows (s)", "feed100Secs", 3),
        ("client takes those 100 rows (s)", "clientSync100Secs", 3),
        ("first sync (s)", "firstSyncSecs", 1),
        ("first sync (rows/s)", "rowsPerSec", 0),
    ] {
        let cell = |n: usize| find(sync, n).and_then(|v| v[key].as_f64());
        out.push_str(&format!("| {name} | {} | {} |\n", fmt(cell(a), decimals), fmt(cell(b), decimals)));
    }
    out.push_str("| first sync before step 5 (rows/s; 66 s for 100,000 rows) | 1500 | about 1500 |\n");
    out
}
