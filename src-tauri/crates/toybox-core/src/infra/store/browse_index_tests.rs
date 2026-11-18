//! The browse index against the rules it stands for: after arbitrary sequences of import, edit, delete,
//! hash arrival and rank change across three libraries, its rows equal the ones the reference rule gives, and
//! a session that reads through it answers like one that reads the libraries.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::{json, Value};

use super::browse::{BrowseFilter, Session, Source, Visibility};
use super::browse_index::{ensure, Ensured, INDEX_FILE};
use super::tests::Tmp;
use super::Store;
use crate::domain::catalog_model::{Asset, Catalog};

#[derive(Clone, Debug, PartialEq)]
struct Spec {
    name: String,
    kind: &'static str, // "audio" | "skybox"
    category: String,
    hash: Option<String>,
    tags: Vec<String>,
}

fn asset(id: &str, spec: &Spec) -> Asset {
    let mut value = json!({
        "id": id, "name": spec.name, "fileName": "x.wav", "relPath": format!("library/{id}/x.wav"), "pack": "p", "category": spec.category,
        "thumb": {"state": "missing"}, "user": {"favorite": false, "collections": [], "tags": []}, "tags": spec.tags,
    });
    if spec.kind == "audio" {
        value["kind"] = json!("audio");
        value["audio"] = json!({"file": format!("library/audio/{id}/x.wav"), "format": "wav", "duration": 1.0, "sampleRate": 48000, "channels": 2, "bitDepth": 16, "byteLength": 10, "modifiedNs": "1", "peaks": [0.0]});
        value["format"] = json!("wav");
    } else {
        value["kind"] = json!("plugin");
        value["type"] = json!("skybox");
        value["files"] = json!([format!("library/skybox/{id}/a.hdr")]);
        value["meta"] = json!({});
    }
    if let Some(hash) = &spec.hash {
        value["contentHash"] = json!(hash);
    }
    serde_json::from_value(value).unwrap()
}

struct Lib {
    id: &'static str,
    store: Store,
    rows: BTreeMap<String, Spec>,
    rank: i64,
}

impl Lib {
    fn new(tmp: &Tmp, id: &'static str, rank: i64) -> Lib {
        let dir = tmp.0.join(id);
        Lib { id, store: Store::remote_at(&dir, &dir.join("catalog.json")), rows: BTreeMap::new(), rank }
    }
    fn save(&self) {
        let assets: Vec<Asset> = self.rows.iter().map(|(id, spec)| asset(id, spec)).collect();
        self.store.save_catalog(&Catalog { rigs: vec![], schema_version: crate::config::SCHEMA_VERSION, library_root: String::new(), assets }).unwrap();
    }
}

fn sources(libs: &[Lib]) -> Vec<Source> {
    libs.iter().map(|lib| Source { id: lib.id.to_string(), library: lib.store.library_path().to_path_buf(), user: lib.store.user_path().to_path_buf(), rank: lib.rank }).collect()
}

struct Rng(u64);
impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

fn spec_at(rng: &mut Rng) -> Spec {
    let names = ["Boom", "boom", "Bang", "zap", "Zap", "alpha", "Beta"];
    let tag_pool = ["loud", "short", "warm", "x"];
    let mut tags = Vec::new();
    for _ in 0..rng.next(3) {
        let tag = tag_pool[rng.next(tag_pool.len())].to_string();
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    Spec {
        name: names[rng.next(names.len())].to_string(),
        kind: if rng.next(4) == 0 { "skybox" } else { "audio" },
        category: format!("c{}", rng.next(3)),
        hash: if rng.next(5) == 0 { None } else { Some(format!("h{}", rng.next(6))) },
        tags,
    }
}

type Key = (String, String); // (library, id)

/// The rule, stated directly: x and y of different libraries are copies when the types are equal and both hashes
/// are present and equal, or the ids are equal and either hash is missing. x is shadowed once per copy in a library
/// that ranks before its own.
fn reference_sh(libs: &[Lib]) -> BTreeMap<Key, i64> {
    let mut ordered: Vec<&Lib> = libs.iter().collect();
    ordered.sort_by_key(|lib| lib.rank);
    let mut out = BTreeMap::new();
    for (i, lib) in ordered.iter().enumerate() {
        for (id, spec) in &lib.rows {
            let mut n = 0;
            for better in &ordered[..i] {
                for (other_id, other) in &better.rows {
                    let same_type = spec.kind == other.kind;
                    let by_hash = matches!((&spec.hash, &other.hash), (Some(a), Some(b)) if a == b);
                    let by_id = id == other_id && (spec.hash.is_none() || other.hash.is_none());
                    if same_type && (by_hash || by_id) {
                        n += 1;
                    }
                }
            }
            out.insert((lib.id.to_string(), id.clone()), n);
        }
    }
    out
}

fn type_of(spec: &Spec) -> &'static str {
    spec.kind
}

/// What the index holds.
struct Held {
    merged: BTreeMap<Key, (String, String, i64)>, // (library, id) -> (type, name, sh)
    shadowed: BTreeSet<Key>,
    hidfacets: BTreeMap<(String, String, String), i64>,
}

fn held(index: &PathBuf, libs: &[Lib]) -> Held {
    let conn = Connection::open(index).unwrap();
    let mut ordered: Vec<&Lib> = libs.iter().collect();
    ordered.sort_by_key(|lib| lib.rank);
    let by_pos: HashMap<i64, &str> = ordered.iter().enumerate().map(|(i, lib)| (i as i64, lib.id)).collect();
    let mut merged = BTreeMap::new();
    let mut statement = conn.prepare("SELECT type, name, id, o, sh FROM merged").unwrap();
    for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, i64>(4)?))).unwrap() {
        let (kind, name, id, o, sh) = row.unwrap();
        assert!(merged.insert((by_pos[&o].to_string(), id), (kind, name, sh)).is_none(), "one merged row per item");
    }
    let shadowed: BTreeSet<Key> = conn
        .prepare("SELECT lib, id FROM shadowed")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    let hidfacets: BTreeMap<_, _> = conn
        .prepare("SELECT type, field, value, n FROM hidfacets")
        .unwrap()
        .query_map([], |row| Ok(((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?), row.get::<_, i64>(3)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    Held { merged, shadowed, hidfacets }
}

fn check_index(index: &PathBuf, libs: &[Lib], note: &str) {
    let got = held(index, libs);
    let want = reference_sh(libs);
    let all: BTreeSet<Key> = want.keys().cloned().collect();
    assert_eq!(got.merged.keys().cloned().collect::<BTreeSet<_>>(), all, "{note}: every item is in the index, once");
    for (key, n) in &want {
        let (kind, name, sh) = &got.merged[key];
        let lib = libs.iter().find(|lib| lib.id == key.0).unwrap();
        let spec = &lib.rows[&key.1];
        assert_eq!(kind, type_of(spec), "{note}: type of {key:?}");
        assert_eq!(name, &spec.name, "{note}: name of {key:?}");
        assert_eq!(sh, n, "{note}: sh of {key:?}");
    }
    let want_shadowed: BTreeSet<Key> = want.iter().filter(|(_, n)| **n > 0).map(|(key, _)| key.clone()).collect();
    assert_eq!(got.shadowed, want_shadowed, "{note}: the shadowed rows are the ones with a better copy");
    // The counts the triggers keep equal the counts of the shadowed rows.
    let mut counts: BTreeMap<(String, String, String), i64> = BTreeMap::new();
    for key in &want_shadowed {
        let lib = libs.iter().find(|lib| lib.id == key.0).unwrap();
        let spec = &lib.rows[&key.1];
        let kind = type_of(spec).to_string();
        *counts.entry((kind.clone(), "type".into(), String::new())).or_default() += 1;
        *counts.entry((kind.clone(), "category".into(), spec.category.clone())).or_default() += 1;
        *counts.entry((kind.clone(), "source".into(), String::new())).or_default() += 1;
        *counts.entry((kind.clone(), "publisher".into(), String::new())).or_default() += 1;
        *counts.entry((kind.clone(), "license".into(), String::new())).or_default() += 1;
        let format = if spec.kind == "audio" { "wav" } else { "hdr" };
        *counts.entry((kind.clone(), "format".into(), format.into())).or_default() += 1;
        for tag in &spec.tags {
            *counts.entry((kind.clone(), "tag".into(), tag.clone())).or_default() += 1;
        }
    }
    assert_eq!(got.hidfacets, counts, "{note}: hidden counts");
}

/// A session through the index answers like one without it.
fn check_session(index: &PathBuf, libs: &[Lib], note: &str) {
    let sources = sources(libs);
    for kind in ["audio", "skybox"] {
        let f = BrowseFilter { kind: kind.into(), ..Default::default() };
        let plain = Session::open(&sources, &[]).unwrap();
        let fast = Session::open_with(&sources, &[], Some(index)).unwrap();
        assert!(fast.has_index(), "{note}: the index is used");
        for descending in [false, true] {
            assert_eq!(fast.page(&f, descending, 0, 10_000).unwrap(), plain.page(&f, descending, 0, 10_000).unwrap(), "{note}: {kind} list (descending {descending})");
        }
        assert_eq!(fast.count(&f).unwrap(), plain.count(&f).unwrap(), "{note}: {kind} count");
        assert_eq!(fast.count(&f).unwrap() as usize, fast.page(&f, false, 0, 10_000).unwrap().len(), "{note}: the count is the length of the list");
        let a = fast.facet_counts(kind, &Visibility::default(), 60, &[]).unwrap();
        let b = plain.facet_counts(kind, &Visibility::default(), 60, &[]).unwrap();
        assert_eq!(serde_json::to_value(&a).unwrap(), serde_json::to_value(&b).unwrap(), "{note}: {kind} facets");
        let ta = fast.type_counts(&Visibility::default()).unwrap();
        let tb = plain.type_counts(&Visibility::default()).unwrap();
        assert_eq!(ta, tb, "{note}: type counts");
        // One library of the three.
        for only in ["a", "b", "c"] {
            let fast = Session::open_with(&sources, &[only.to_string()], Some(index)).unwrap();
            let plain = Session::open(&sources, &[only.to_string()]).unwrap();
            assert_eq!(fast.page(&f, false, 0, 10_000).unwrap(), plain.page(&f, false, 0, 10_000).unwrap(), "{note}: only {only}");
        }
    }
}

#[test]
fn marks_equal_the_rule_after_any_sequence_of_writes() {
    let tmp = Tmp::new("ix-sequence");
    let mut libs = vec![Lib::new(&tmp, "a", 0), Lib::new(&tmp, "b", 1), Lib::new(&tmp, "c", 2)];
    let index = tmp.0.join(INDEX_FILE);
    let mut rng = Rng(7);
    let mut reorders = 0;
    for lib in &libs {
        lib.save();
    }
    assert_eq!(ensure(&index, &sources(&libs), true).unwrap(), Ensured::Ready);
    check_index(&index, &libs, "empty");
    for step in 0..120 {
        // A few operations between catch-ups, so batches of several changes are absorbed at once.
        let batch = 1 + rng.next(4);
        let mut note = String::new();
        for _ in 0..batch {
            let which = rng.next(3);
            let ids: Vec<String> = libs[which].rows.keys().cloned().collect();
            match rng.next(10) {
                0..=3 => {
                    // import (or replace)
                    let id = format!("i{}", rng.next(10));
                    let spec = spec_at(&mut rng);
                    libs[which].rows.insert(id.clone(), spec);
                    note.push_str(&format!("import {id} into {}; ", libs[which].id));
                }
                4 | 5 if !ids.is_empty() => {
                    // edit name/category/tags, keep the hash
                    let id = ids[rng.next(ids.len())].clone();
                    let edit = spec_at(&mut rng);
                    let row = libs[which].rows.get_mut(&id).unwrap();
                    row.name = edit.name;
                    row.category = edit.category;
                    row.tags = edit.tags;
                    note.push_str(&format!("edit {id} in {}; ", libs[which].id));
                }
                6 | 7 if !ids.is_empty() => {
                    let id = ids[rng.next(ids.len())].clone();
                    libs[which].rows.remove(&id);
                    note.push_str(&format!("delete {id} from {}; ", libs[which].id));
                }
                8 if !ids.is_empty() => {
                    // hash arrival (or change)
                    let id = ids[rng.next(ids.len())].clone();
                    libs[which].rows.get_mut(&id).unwrap().hash = Some(format!("h{}", rng.next(6)));
                    note.push_str(&format!("hash {id} in {}; ", libs[which].id));
                }
                9 if step % 15 == 14 => {
                    // rank change: the libraries are reordered
                    let mut ranks: Vec<i64> = vec![0, 1, 2];
                    for i in (1..3).rev() {
                        ranks.swap(i, rng.next(i + 1));
                    }
                    for (lib, rank) in libs.iter_mut().zip(ranks) {
                        lib.rank = rank;
                    }
                    reorders += 1;
                    note.push_str("reorder; ");
                }
                _ => {}
            }
            libs[which].save();
        }
        assert_eq!(ensure(&index, &sources(&libs), true).unwrap(), Ensured::Ready);
        check_index(&index, &libs, &format!("step {step}: {note}"));
        if step % 10 == 0 {
            check_session(&index, &libs, &format!("step {step}"));
        }
    }
    check_session(&index, &libs, "end");
    // Everything but the first build and the rank changes was absorbed row by row.
    let builds = super::browse_index::REBUILDS.with(|count| count.get());
    assert!(builds <= 1 + reorders, "{builds} full builds for {reorders} rank changes");
    assert!(reorders >= 1, "the sequence changed ranks at least once");
}

#[test]
fn a_library_added_or_removed_rebuilds_and_agrees() {
    let tmp = Tmp::new("ix-membership");
    let mut libs = vec![Lib::new(&tmp, "a", 0), Lib::new(&tmp, "b", 1), Lib::new(&tmp, "c", 2)];
    let mut rng = Rng(99);
    for lib in libs.iter_mut() {
        for i in 0..25 {
            lib.rows.insert(format!("i{}", i % 12), spec_at(&mut rng));
        }
        lib.save();
    }
    let index = tmp.0.join(INDEX_FILE);
    // Only two of the three first.
    let two = sources(&libs[..2]);
    ensure(&index, &two, true).unwrap();
    check_index(&index, &libs[..2], "two libraries");
    ensure(&index, &sources(&libs), true).unwrap();
    check_index(&index, &libs, "three libraries");
    ensure(&index, &sources(&libs[1..]), true).unwrap();
    check_index(&index, &libs[1..], "the best one removed");
}

#[test]
fn a_write_that_left_no_trace_in_the_feed_is_caught_by_the_counts() {
    let tmp = Tmp::new("ix-drift");
    let mut libs = vec![Lib::new(&tmp, "a", 0), Lib::new(&tmp, "b", 1)];
    let mut rng = Rng(5);
    for lib in libs.iter_mut() {
        for i in 0..10 {
            lib.rows.insert(format!("i{i}"), spec_at(&mut rng));
        }
        lib.save();
    }
    let index = tmp.0.join(INDEX_FILE);
    ensure(&index, &sources(&libs), true).unwrap();
    // A row appears in the library without a `changes` row or a revision.
    let spec = Spec { name: "Sneaky".into(), kind: "audio", category: "c0".into(), hash: Some("h0".into()), tags: vec![] };
    let rogue = asset("rogue", &spec);
    libs[1].rows.insert("rogue".into(), spec);
    {
        let conn = Connection::open(libs[1].store.library_path()).unwrap();
        let text = serde_json::to_string(&rogue).unwrap();
        conn.execute(
            "INSERT INTO assets(id,item_id,ord,kind,type,name,pack,category,format,source,publisher,license,content_hash,hash_key,asset,revision,rel_path,thumb_name,clip_source,plugin) VALUES('rogue',NULL,999,'audio','audio','Sneaky','p','c0','wav','','','','h0',NULL,?1,1,'','rogue','','')",
            [text],
        )
        .unwrap();
    }
    ensure(&index, &sources(&libs), true).unwrap();
    check_index(&index, &libs, "after a write the feed missed");
}

#[test]
fn deep_pages_equal_the_reference_order_at_any_depth() {
    let tmp = Tmp::new("ix-depth");
    let mut libs = vec![Lib::new(&tmp, "a", 0), Lib::new(&tmp, "b", 1), Lib::new(&tmp, "c", 2)];
    let mut rng = Rng(3);
    for (n, lib) in libs.iter_mut().enumerate() {
        for i in 0..400 {
            let mut spec = spec_at(&mut rng);
            spec.kind = "audio";
            // Every library holds its own rows, and copies of the first library's.
            let id = if i % 4 == 0 { format!("shared{}", i / 4) } else { format!("{}{i}", lib.id) };
            if i % 4 == 0 {
                spec.hash = Some(format!("hs{}", i / 4));
                spec.name = format!("Shared {}", i / 4);
            } else {
                spec.hash = Some(format!("{}h{i}", lib.id));
                spec.name = format!("{} {}", ["tone", "Tone", "TONE"][n], i % 37);
            }
            lib.rows.insert(id, spec);
        }
        lib.save();
    }
    let index = tmp.0.join(INDEX_FILE);
    assert_eq!(ensure(&index, &sources(&libs), true).unwrap(), Ensured::Ready);
    let f = BrowseFilter { kind: "audio".into(), ..Default::default() };
    let fast = Session::open_with(&sources(&libs), &[], Some(&index)).unwrap();
    assert!(fast.has_index());
    // The reference: every row not shadowed, sorted by (name nocase, id, library position).
    let sh = reference_sh(&libs);
    let mut rows: Vec<(String, String, usize)> = Vec::new();
    for (pos, lib) in libs.iter().enumerate() {
        for (id, spec) in &lib.rows {
            if sh[&(lib.id.to_string(), id.clone())] == 0 {
                rows.push((spec.name.to_lowercase(), id.clone(), pos));
            }
        }
    }
    rows.sort();
    let want: Vec<(String, String)> = rows.iter().map(|(_, id, pos)| (libs[*pos].id.to_string(), id.clone())).collect();
    assert_eq!(fast.count(&f).unwrap() as usize, want.len());
    for offset in [0usize, 1, 99, 100, 101, 333, want.len() / 2, want.len() - 100, want.len() - 7, want.len() - 1] {
        let got = fast.page(&f, false, offset as i64, 100).unwrap();
        assert_eq!(got, want[offset..(offset + 100).min(want.len())].to_vec(), "page at {offset}");
    }
    let reversed: Vec<(String, String)> = want.iter().rev().cloned().collect();
    for offset in [0usize, 250, want.len() - 5] {
        assert_eq!(fast.page(&f, true, offset as i64, 100).unwrap(), reversed[offset..(offset + 100).min(want.len())].to_vec(), "descending page at {offset}");
    }
    // The plan is a walk of one table in key order, nothing sorted.
    let plan = fast.explain_index_page(&f, 1000, 100).unwrap().join(" | ");
    assert!(plan.contains("merged") && !plan.contains("TEMP B-TREE"), "plan: {plan}");
}

#[test]
fn a_build_that_takes_long_runs_in_the_background_and_the_list_is_the_same_meanwhile() {
    let tmp = Tmp::new("ix-background");
    let mut libs = vec![Lib::new(&tmp, "a", 0), Lib::new(&tmp, "b", 1)];
    let mut rng = Rng(11);
    for lib in libs.iter_mut() {
        for i in 0..1_200 {
            let mut spec = spec_at(&mut rng);
            spec.kind = "audio";
            lib.rows.insert(format!("{}{i}", lib.id), spec);
        }
        lib.save();
    }
    let index = tmp.0.join(INDEX_FILE);
    let srcs = sources(&libs);
    // Not built yet: the caller is told so at once, and a session without the index gives the list.
    assert_eq!(ensure(&index, &srcs, false).unwrap(), Ensured::Pending);
    let f = BrowseFilter { kind: "audio".into(), ..Default::default() };
    let meanwhile = Session::open_with(&srcs, &[], Some(&index)).unwrap().page(&f, false, 0, 50).unwrap();
    let mut ready = false;
    for _ in 0..200 {
        if ensure(&index, &srcs, false).unwrap() == Ensured::Ready {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(ready, "the background build finishes");
    let after = Session::open_with(&srcs, &[], Some(&index)).unwrap();
    assert!(after.has_index());
    assert_eq!(after.page(&f, false, 0, 50).unwrap(), meanwhile);
    // A small change is absorbed on the caller's thread.
    libs[0].rows.remove("a5");
    libs[0].save();
    assert_eq!(ensure(&index, &srcs, false).unwrap(), Ensured::Ready);
    check_index(&index, &libs, "after a small change");
}

#[test]
fn values_read_back_are_the_values_written_for_unused_helpers() {
    // Keeps the helper used in this module honest: the asset JSON round-trips.
    let spec = Spec { name: "n".into(), kind: "skybox", category: "c".into(), hash: None, tags: vec!["t".into()] };
    let value: Value = serde_json::to_value(asset("x", &spec)).unwrap();
    assert_eq!(value["id"], "x");
}
