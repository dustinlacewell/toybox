//! The window's queries against the rules of the window's own code
//! (`applyFilter`, `dedupeAssets`, the facet counts), ported here as a reference and
//! run on a fixture of three libraries with copies across them.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{json, Value};

use super::browse::{BrowseFilter, Session, Source, Visibility};
use super::tests::Tmp;
use super::Store;
use crate::domain::catalog_model::{Asset, Catalog};

struct Lib {
    id: &'static str,
    rank: i64,
    store: Store,
    assets: Vec<Asset>,
}

#[allow(clippy::too_many_arguments)]
fn audio(id: &str, name: &str, category: &str, hash: Option<&str>, tags: &[&str], personal: &[&str], favorite: bool, collections: &[&str], provenance: Option<(&str, &str)>, kind: &str) -> Value {
    let mut user_tags: Vec<String> = Vec::new();
    let mut user = json!({"favorite": favorite, "collections": collections});
    if let Some((source, license)) = provenance {
        user["provenance"] = json!({"source": source, "license": license});
        user_tags.push(format!("source: {source}"));
        user_tags.push(format!("license: {license}"));
    }
    user_tags.extend(personal.iter().map(|tag| tag.to_string()));
    user["tags"] = json!(user_tags);
    let mut value = json!({
        "id": id, "name": name, "fileName": "x.wav", "relPath": format!("library/{kind}/{id}/x.wav"), "pack": "p", "category": category,
        "thumb": {"state": "missing"}, "user": user, "tags": tags,
    });
    if kind == "audio" {
        value["kind"] = json!("audio");
        value["audio"] = json!({"file": format!("library/audio/{id}/x.wav"), "format": "wav", "duration": 1.0, "sampleRate": 48000, "channels": 2, "bitDepth": 16, "byteLength": 10, "modifiedNs": "1", "peaks": [0.0]});
        value["format"] = json!("wav");
    } else {
        value["kind"] = json!("plugin");
        value["type"] = json!(kind);
        value["files"] = json!([format!("library/{kind}/{id}/a.hdr")]);
        value["meta"] = json!({});
    }
    if let Some(hash) = hash {
        value["contentHash"] = json!(hash);
    }
    value
}

fn library(tmp: &Tmp, id: &'static str, rank: i64, assets: Vec<Value>) -> Lib {
    let dir = tmp.0.join(id);
    let store = Store::remote_at(&dir, &dir.join("catalog.json"));
    let assets: Vec<Asset> = assets.into_iter().map(|value| serde_json::from_value(value).unwrap()).collect();
    store.save_catalog(&Catalog { rigs: vec![], schema_version: crate::config::SCHEMA_VERSION, library_root: String::new(), assets: assets.clone() }).unwrap();
    Lib { id, rank, store, assets }
}

/// Three libraries: `a` (best), `b`, `c`. Copies across them by hash, by id (no hash on a side), and
/// the same id with different content.
fn fixture(tmp: &Tmp) -> Vec<Lib> {
    let a = vec![
        audio("a1", "Boom", "sfx", Some("h1"), &["loud", "explosion"], &["mine"], true, &["Zoo"], Some(("Pack A", "CC0")), "audio"),
        audio("a2", "bang", "sfx", Some("h2"), &["loud", "short"], &[], false, &[], Some(("Pack A", "CC0")), "audio"),
        audio("a3", "Zap", "laser", None, &["short"], &[], false, &[], None, "audio"),
        audio("a4", "ab", "laser", Some("h4"), &[], &["short"], false, &["Zoo"], None, "audio"),
        audio("a5", "Boom", "sfx", Some("h5"), &["Source: Hidden Tag"], &[], false, &[], None, "audio"),
        audio("s1", "Dusk", "sky", Some("hs1"), &["warm"], &[], true, &[], None, "skybox"),
    ];
    let b = vec![
        audio("b1", "Boom copy", "sfx", Some("h1"), &["loud"], &[], false, &[], Some(("Pack B", "MIT")), "audio"),
        audio("a2", "bang v2", "sfx", Some("h2x"), &["loud"], &[], true, &["Zoo"], Some(("Pack A", "CC0")), "audio"),
        audio("a3", "Zap again", "laser", None, &["short"], &[], false, &[], None, "audio"),
        audio("b4", "Bz", "misc", Some("h9"), &["buzz"], &[], false, &[], None, "audio"),
        audio("b5", "Dusk copy", "sky", Some("hs1"), &[], &[], false, &[], None, "skybox"),
        audio("b6", "Mist", "sky", Some("hs6"), &[], &[], false, &[], None, "skybox"),
    ];
    let c = vec![
        audio("c1", "Boom third", "sfx", Some("h1"), &["loud"], &[], false, &[], None, "audio"),
        audio("b4", "Bz", "misc", None, &["buzz"], &[], false, &[], None, "audio"),
        audio("c3", "Cymbal", "sfx", Some("h3"), &["loud"], &["mine", "short"], true, &["Zoo", "Crash"], None, "audio"),
        audio("c4", "bang", "sfx", Some("h4"), &[], &[], false, &[], None, "audio"),
    ];
    vec![library(tmp, "a", 0, a), library(tmp, "b", 1, b), library(tmp, "c", 2, c)]
}

fn sources(libs: &[Lib]) -> Vec<Source> {
    libs.iter().map(|lib| Source { id: lib.id.to_string(), library: lib.store.library_path().to_path_buf(), user: lib.store.user_path().to_path_buf(), rank: lib.rank }).collect()
}

// ---- the window's rules, ported ------------------------------------------------------

type Key = (String, String); // (library, id)

fn type_of(asset: &Asset) -> String {
    asset.content.type_id().to_string()
}

fn provenance_value(asset: &Asset, field: &str) -> String {
    let recorded = asset.user.provenance.as_ref().and_then(|p| match field {
        "source" => p.source.clone(),
        "publisher" => p.publisher.clone(),
        _ => p.license.clone(),
    });
    if let Some(value) = recorded.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        return value;
    }
    let prefix = format!("{field}:");
    asset.user.tags.iter().find(|tag| tag.to_lowercase().starts_with(&prefix)).map(|tag| tag[prefix.len()..].trim().to_string()).unwrap_or_default()
}

/// `assetFormat` of src/domain/facets.ts, for the kinds of the fixture.
fn format_of(asset: &Asset) -> String {
    use crate::domain::catalog_model::AssetContent;
    if let Some(format) = asset.format.as_deref().map(|f| f.trim().trim_start_matches('.').to_lowercase()).filter(|f| !f.is_empty()) {
        return format;
    }
    let file = match &asset.content {
        AssetContent::Plugin { files, .. } => files.first().cloned().unwrap_or_default(),
        AssetContent::Audio { audio } => audio.file.clone(),
        _ => String::new(),
    };
    file.rsplit('.').next().filter(|ext| *ext != file).unwrap_or("").to_lowercase()
}

fn all_tags(asset: &Asset) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in asset.tags.iter().chain(&asset.user.tags) {
        if !out.contains(tag) {
            out.push(tag.clone());
        }
    }
    out
}

fn hidden_by_plugin(asset: &Asset, visibility: &Visibility) -> bool {
    visibility.hidden_types.contains(&type_of(asset))
}

/// `link` of src/domain/duplicates.ts: the assets that stay visible, among `libs` (in rank order).
fn dedupe_js(libs: &[(&str, Vec<&Asset>)]) -> HashSet<Key> {
    #[derive(Clone)]
    struct Entry {
        key: Key,
        lib: usize,
        seq: usize,
    }
    struct Slot {
        lib: usize,
        first: Entry,
        by_raw: Option<HashMap<String, Entry>>,
    }
    fn remember(index: &mut HashMap<String, Slot>, key: String, entry: Entry, raw: Option<&str>) {
        match index.get_mut(&key) {
            None => {
                index.insert(key, Slot { lib: entry.lib, first: entry.clone(), by_raw: raw.map(|raw| HashMap::from([(raw.to_string(), entry)])) });
            }
            Some(slot) => {
                if slot.lib == entry.lib {
                    if let (Some(raw), Some(by_raw)) = (raw, slot.by_raw.as_mut()) {
                        by_raw.entry(raw.to_string()).or_insert(entry);
                    }
                }
            }
        }
    }
    let better = |a: &(Entry, bool), b: &Option<(Entry, bool)>| match b {
        None => true,
        Some(b) => a.0.lib < b.0.lib || (a.0.lib == b.0.lib && ((a.1 && !b.1) || (a.1 == b.1 && a.0.seq < b.0.seq))),
    };
    let mut by_hash: HashMap<String, Slot> = HashMap::new();
    let mut by_raw: HashMap<String, Slot> = HashMap::new();
    let mut unhashed: HashMap<String, Slot> = HashMap::new();
    let mut shown: HashSet<Key> = HashSet::new();
    let mut seq = 0;
    for (lib, (id, assets)) in libs.iter().enumerate() {
        let mut added: Vec<(Entry, String, String, Option<String>)> = Vec::new();
        for asset in assets {
            let kind = type_of(asset);
            let raw = asset.id.clone();
            let hash = asset.content_hash.clone();
            let raw_key = format!("{kind}\0{raw}");
            let mut best: Option<(Entry, bool)> = None;
            if let Some(hash) = &hash {
                if let Some(slot) = by_hash.get(&format!("{kind}\0{hash}")) {
                    let by = slot.by_raw.as_ref().unwrap();
                    let candidate = (by.get(&raw).cloned().unwrap_or_else(|| slot.first.clone()), by.contains_key(&raw));
                    if better(&candidate, &best) {
                        best = Some(candidate);
                    }
                }
                if let Some(open) = unhashed.get(&raw_key) {
                    let candidate = (open.first.clone(), true);
                    if better(&candidate, &best) {
                        best = Some(candidate);
                    }
                }
            } else if let Some(any) = by_raw.get(&raw_key) {
                best = Some((any.first.clone(), true));
            }
            if best.is_none() {
                let entry = Entry { key: (id.to_string(), asset.id.clone()), lib, seq };
                seq += 1;
                shown.insert(entry.key.clone());
                added.push((entry, kind, raw, hash));
            }
        }
        for (entry, kind, raw, hash) in added {
            match &hash {
                Some(hash) => remember(&mut by_hash, format!("{kind}\0{hash}"), entry.clone(), Some(&raw)),
                None => remember(&mut unhashed, format!("{kind}\0{raw}"), entry.clone(), None),
            }
            remember(&mut by_raw, format!("{kind}\0{raw}"), entry, None);
        }
    }
    shown
}

struct Reference<'a> {
    libs: &'a [Lib],
}

impl Reference<'_> {
    /// Visible assets of every library (the window's `assets`), in library order.
    fn visible(&self, visibility: &Visibility) -> Vec<(usize, &Asset)> {
        self.libs.iter().enumerate().flat_map(|(i, lib)| lib.assets.iter().filter(|a| !hidden_by_plugin(a, visibility)).map(move |a| (i, a))).collect()
    }

    /// `unique.assets` when `only` is empty, else the grid's `dedupeAssets(.., only)`.
    fn deduped(&self, visibility: &Visibility, only: &[String]) -> Vec<(usize, &Asset)> {
        let visible = self.visible(visibility);
        let mut order: Vec<usize> = (0..self.libs.len()).filter(|i| only.is_empty() || only.contains(&self.libs[*i].id.to_string())).collect();
        order.sort_by_key(|i| (self.libs[*i].rank, *i));
        let groups: Vec<(&str, Vec<&Asset>)> = order.iter().map(|i| (self.libs[*i].id, visible.iter().filter(|(l, _)| l == i).map(|(_, a)| *a).collect())).collect();
        let shown = dedupe_js(&groups);
        visible.into_iter().filter(|(i, a)| shown.contains(&(self.libs[*i].id.to_string(), a.id.clone()))).collect()
    }

    fn list(&self, f: &BrowseFilter) -> Vec<Key> {
        let needle = f.text.trim().to_lowercase();
        let mut rows: Vec<(String, String, usize, Key)> = self
            .deduped(&f.visibility, &f.libraries)
            .into_iter()
            .filter(|(_, a)| {
                type_of(a) == f.kind
                    && (f.sources.is_empty() || f.sources.contains(&provenance_value(a, "source")))
                    && (f.publishers.is_empty() || f.publishers.contains(&provenance_value(a, "publisher")))
                    && (f.licenses.is_empty() || f.licenses.contains(&provenance_value(a, "license")))
                    && (f.formats.is_empty() || f.formats.contains(&format_of(a)))
                    && (f.categories.is_empty() || f.categories.contains(&a.category))
                    && (!f.favorites_only || a.user.favorite)
                    && f.tags.iter().all(|tag| all_tags(a).contains(tag))
                    && (needle.is_empty() || std::iter::once(a.name.clone()).chain(all_tags(a)).any(|v| v.to_lowercase().contains(&needle)))
                    && f.collection.as_ref().is_none_or(|c| a.user.collections.contains(c))
            })
            .map(|(i, a)| (a.name.to_lowercase(), a.id.clone(), i, (self.libs[i].id.to_string(), a.id.clone())))
            .collect();
        rows.sort_by(|x, y| (&x.0, &x.1, x.2).cmp(&(&y.0, &y.1, y.2)));
        rows.into_iter().map(|row| row.3).collect()
    }
}

fn filter(kind: &str) -> BrowseFilter {
    BrowseFilter { kind: kind.into(), ..Default::default() }
}

fn session(libs: &[Lib], f: &BrowseFilter) -> Session {
    Session::open(&sources(libs), &f.libraries).unwrap()
}

fn page_all(libs: &[Lib], f: &BrowseFilter) -> Vec<Key> {
    session(libs, f).page(f, false, 0, 1000).unwrap()
}

fn check(libs: &[Lib], f: &BrowseFilter, what: &str) {
    let expected = Reference { libs }.list(f);
    let session = session(libs, f);
    let got = session.page(f, false, 0, 1000).unwrap();
    assert_eq!(got, expected, "page: {what}");
    assert_eq!(session.count(f).unwrap(), expected.len() as i64, "count: {what}");
    // Paging: the pieces put together are the list.
    let mut pieces = Vec::new();
    for offset in (0..expected.len() as i64 + 2).step_by(2) {
        pieces.extend(session.page(f, false, offset, 2).unwrap());
    }
    assert_eq!(pieces, expected, "pages of 2: {what}");
    let mut reversed = expected.clone();
    reversed.reverse();
    let descending = session.page(f, true, 0, 1000).unwrap();
    assert_eq!(descending.len(), reversed.len(), "descending length: {what}");
}

#[test]
fn the_list_matches_the_rules_of_the_window() {
    let tmp = Tmp::new("browse-list");
    let libs = fixture(&tmp);
    check(&libs, &filter("audio"), "every audio item");
    check(&libs, &filter("skybox"), "skyboxes");
    let mut f;
    for (what, edit) in [
        ("category", (|f: &mut BrowseFilter| f.categories = vec!["sfx".into()]) as fn(&mut BrowseFilter)),
        ("two categories", |f| f.categories = vec!["sfx".into(), "laser".into()]),
        ("a source", |f| f.sources = vec!["Pack A".into()]),
        ("no source", |f| f.sources = vec![String::new()]),
        ("a license", |f| f.licenses = vec!["MIT".into()]),
        ("a format", |f| f.formats = vec!["wav".into()]),
        ("favourites", |f| f.favorites_only = true),
        ("a collection", |f| f.collection = Some("Zoo".into())),
        ("a collection and favourites", |f| {
            f.collection = Some("Zoo".into());
            f.favorites_only = true;
        }),
        ("a library tag", |f| f.tags = vec!["loud".into()]),
        ("a personal tag", |f| f.tags = vec!["mine".into()]),
        ("a tag both ways", |f| f.tags = vec!["short".into()]),
        ("two tags", |f| f.tags = vec!["loud".into(), "mine".into()]),
        ("a tag that looks like provenance", |f| f.tags = vec!["Source: Hidden Tag".into()]),
        ("one character", |f| f.text = "z".into()),
        ("two characters", |f| f.text = "ba".into()),
        ("two characters of a tag", |f| f.text = "ex".into()),
        ("two characters of a personal tag", |f| f.text = "mi".into()),
        ("three characters", |f| f.text = "boo".into()),
        ("a longer word", |f| f.text = "BOOM".into()),
        ("a tag word", |f| f.text = "loud".into()),
        ("a personal tag word", |f| f.text = "mine".into()),
        ("nothing matches", |f| f.text = "qqqq".into()),
        ("padded text", |f| f.text = "  zap ".into()),
        ("percent and underscore", |f| f.text = "100%".into()),
        ("text with a quote", |f| f.text = "\"boom".into()),
        ("text and a tag and a category", |f| {
            f.text = "bo".into();
            f.tags = vec!["loud".into()];
            f.categories = vec!["sfx".into()];
        }),
        ("libraries b and c", |f| f.libraries = vec!["b".into(), "c".into()]),
        ("library c", |f| f.libraries = vec!["c".into()]),
        ("library b and a collection", |f| {
            f.libraries = vec!["b".into()];
            f.collection = Some("Zoo".into());
        }),
    ] {
        f = filter("audio");
        edit(&mut f);
        check(&libs, &f, what);
    }
    // A switched-off type hides its items, and they no longer hide copies.
    let mut f = filter("skybox");
    f.visibility.hidden_types = vec!["skybox".into()];
    check(&libs, &f, "hidden type");
    let mut f = filter("audio");
    f.visibility.plugins_off = vec!["gone".into()];
    check(&libs, &f, "a plugin that is off");
}

#[test]
fn copies_are_hidden_in_the_better_library_and_counted_the_same() {
    let tmp = Tmp::new("browse-dupes");
    let libs = fixture(&tmp);
    let f = filter("audio");
    let ids: HashSet<Key> = page_all(&libs, &f).into_iter().collect();
    // b1 and c1 are copies of a1 by hash; b4's copy in c has no hash, so the id decides; a3 and b's a3 likewise.
    for hidden in [("b", "b1"), ("c", "c1"), ("c", "b4"), ("b", "a3")] {
        assert!(!ids.contains(&(hidden.0.to_string(), hidden.1.to_string())), "{hidden:?} is hidden");
    }
    // The same id with another content is another item, shown.
    assert!(ids.contains(&("b".to_string(), "a2".to_string())));
    // Chosen libraries show their own copies, de-duplicated among themselves.
    let mut only = filter("audio");
    only.libraries = vec!["b".into(), "c".into()];
    let ids: HashSet<Key> = page_all(&libs, &only).into_iter().collect();
    assert!(ids.contains(&("b".to_string(), "b1".to_string())));
    assert!(!ids.contains(&("c".to_string(), "c1".to_string())));
}

#[test]
fn counts_agree_with_the_grid() {
    let tmp = Tmp::new("browse-counts");
    let libs = fixture(&tmp);
    let reference = Reference { libs: &libs };
    let none = Visibility::default();
    let mut hiding = Visibility::default();
    hiding.hidden_types = vec!["skybox".into()];
    for visibility in [none, hiding] {
        let session = Session::open(&sources(&libs), &[]).unwrap();
        let unique = reference.deduped(&visibility, &[]);
        // Sidebar.
        let mut per_type: BTreeMap<String, i64> = BTreeMap::new();
        for (_, a) in &unique {
            *per_type.entry(type_of(a)).or_insert(0) += 1;
        }
        let counts: BTreeMap<String, i64> = session.type_counts(&visibility).unwrap().into_iter().map(|c| (c.value, c.count)).collect();
        assert_eq!(counts, per_type, "type counts");
        for kind in ["audio", "skybox"] {
            let of_kind: Vec<&(usize, &Asset)> = unique.iter().filter(|(_, a)| type_of(a) == kind).collect();
            let facets = session.facet_counts(kind, &visibility, 100, &[]).unwrap();
            assert_eq!(facets.total, of_kind.len() as i64, "total of {kind}");
            let tally = |field: &dyn Fn(&Asset) -> String| {
                let mut map: BTreeMap<String, i64> = BTreeMap::new();
                for (_, a) in &of_kind {
                    *map.entry(field(a)).or_insert(0) += 1;
                }
                map
            };
            let as_map = |counts: &[super::browse::Count]| counts.iter().map(|c| (c.value.clone(), c.count)).collect::<BTreeMap<_, _>>();
            assert_eq!(as_map(&facets.categories), tally(&|a| a.category.clone()), "categories of {kind}");
            assert_eq!(as_map(&facets.sources), tally(&|a| provenance_value(a, "source")), "sources of {kind}");
            assert_eq!(as_map(&facets.licenses), tally(&|a| provenance_value(a, "license")), "licenses of {kind}");
            assert_eq!(as_map(&facets.publishers), tally(&|a| provenance_value(a, "publisher")), "publishers of {kind}");
            assert_eq!(as_map(&facets.formats), tally(&|a| format_of(a)), "formats of {kind}");
            let mut tags: BTreeMap<String, i64> = BTreeMap::new();
            for (_, a) in &of_kind {
                for tag in all_tags(a) {
                    let lower = tag.to_lowercase();
                    if !(lower.starts_with("source:") || lower.starts_with("publisher:") || lower.starts_with("license:")) {
                        *tags.entry(tag).or_insert(0) += 1;
                    }
                }
            }
            assert_eq!(as_map(&facets.tags), tags, "tags of {kind}");
            assert_eq!(facets.tag_total, tags.len() as i64);
            // Every copy per library, visible ones.
            let mut per_library: BTreeMap<String, i64> = libs.iter().map(|lib| (lib.id.to_string(), 0)).collect();
            for (i, a) in reference.visible(&visibility) {
                if type_of(a) == kind {
                    *per_library.entry(libs[i].id.to_string()).or_insert(0) += 1;
                }
            }
            assert_eq!(as_map(&facets.libraries), per_library, "copies per library of {kind}");
        }
        // Collections: names of unique shown items, counts of the type.
        let mut names: BTreeMap<String, i64> = BTreeMap::new();
        for (_, a) in &unique {
            for name in &a.user.collections {
                *names.entry(name.clone()).or_insert(0) += (type_of(a) == "audio") as i64;
            }
        }
        let got: BTreeMap<String, i64> = session.collections("audio", &visibility).unwrap().into_iter().map(|c| (c.name, c.count)).collect();
        assert_eq!(got, names, "collections");
    }
}

#[test]
fn tags_are_limited_and_searchable() {
    let tmp = Tmp::new("browse-tags");
    let libs = fixture(&tmp);
    let session = Session::open(&sources(&libs), &[]).unwrap();
    let none = Visibility::default();
    let facets = session.facet_counts("audio", &none, 2, &["explosion".to_string()]).unwrap();
    let names: Vec<&str> = facets.tags.iter().map(|c| c.value.as_str()).collect();
    assert_eq!(facets.tags.len(), 3, "the two most used and the selected one: {names:?}");
    assert!(names.contains(&"loud") && names.contains(&"explosion"));
    assert_eq!(facets.tag_total, 5);
    let found = session.search_tags("audio", &none, "SHO", 10).unwrap();
    assert_eq!(found.iter().map(|c| c.value.as_str()).collect::<Vec<_>>(), ["short"]);
    assert!(session.search_tags("audio", &none, "source", 10).unwrap().is_empty(), "provenance-looking tags are not offered");
}

#[test]
fn where_else_an_item_lives() {
    let tmp = Tmp::new("browse-elsewhere");
    let libs = fixture(&tmp);
    let session = Session::open(&sources(&libs), &[]).unwrap();
    let found = session.elsewhere(&[("a".into(), "a1".into()), ("c".into(), "b4".into()), ("b".into(), "a2".into())], &Visibility::default()).unwrap();
    let copies = |entry: &super::browse::Elsewhere| entry.also_in.iter().map(|c| format!("{}:{}", c.library, c.id)).collect::<Vec<_>>();
    assert_eq!(copies(&found[0]), ["b:b1", "c:c1"]);
    assert_eq!(copies(&found[1]), ["b:b4"], "an unhashed copy matches by id");
    assert!(found[2].also_in.is_empty());
    assert_eq!(found[2].other_versions.iter().map(|c| format!("{}:{}", c.library, c.id)).collect::<Vec<_>>(), ["a:a2"], "same id, different content");
}

#[test]
fn the_plans_use_indexes() {
    let tmp = Tmp::new("browse-plans");
    let libs = fixture(&tmp);
    let plan = |f: &BrowseFilter| {
        let session = session(&libs, f);
        session.explain_page(f, 0, 50).unwrap().join("\n")
    };
    let first = plan(&filter("audio"));
    assert!(first.contains("assets_browse"), "the list walks the browse index:\n{first}");
    assert!(!first.contains("SCAN a\n") && !first.contains("SCAN a "), "no full scan of assets:\n{first}");
    let mut f = filter("audio");
    f.tags = vec!["loud".into()];
    let tagged = plan(&f);
    assert!(tagged.contains("SCAN d") && !tagged.contains("SCAN a"), "a tag starts from its rows, not from the list:
{tagged}");
    let mut f = filter("audio");
    f.text = "boom".into();
    let text = plan(&f);
    assert!(text.contains("SCAN d") && !text.contains("SCAN a"), "a search starts from its matches:
{text}");
    let mut f = filter("audio");
    f.collection = Some("Zoo".into());
    let collected = plan(&f);
    assert!(collected.contains("SCAN d") && !collected.contains("SCAN a"), "a collection starts from its members:
{collected}");
    let mut f = filter("audio");
    f.favorites_only = true;
    let fav = plan(&f);
    assert!(fav.contains("SCAN d") && !fav.contains("SCAN a"), "favourites start from their rows:
{fav}");
    // The copy probes use the hash index and the primary key.
    assert!(first.contains("assets_hash") || first.contains("sqlite_autoindex_assets"), "the copy probe uses an index:\n{first}");
}

#[test]
fn facet_counts_follow_writes() {
    let tmp = Tmp::new("browse-triggers");
    let libs = fixture(&tmp);
    let store = &libs[0].store;
    let before = Session::open(&sources(&libs), &["a".to_string()]).unwrap().facet_counts("audio", &Visibility::default(), 100, &[]).unwrap();
    assert_eq!(before.total, 5);
    // Remove an item and change another's tags and category through the whole-document writer.
    let mut catalog = store.load_catalog().unwrap().unwrap();
    catalog.assets.retain(|a| a.id != "a3");
    for asset in &mut catalog.assets {
        if asset.id == "a2" {
            asset.category = "voice".into();
            asset.tags = vec!["quiet".into()];
        }
    }
    store.save_catalog(&catalog).unwrap();
    let after = Session::open(&sources(&libs), &["a".to_string()]).unwrap().facet_counts("audio", &Visibility::default(), 100, &[]).unwrap();
    assert_eq!(after.total, 4);
    let by = |counts: &[super::browse::Count], value: &str| counts.iter().find(|c| c.value == value).map(|c| c.count);
    assert_eq!(by(&after.categories, "voice"), Some(1));
    assert_eq!(by(&after.categories, "laser"), Some(1), "a3 left");
    assert_eq!(by(&after.tags, "quiet"), Some(1));
    assert_eq!(by(&after.tags, "short"), Some(1), "a3's tag left with it; a4 has it as a personal tag");
    assert_eq!(by(&after.tags, "loud"), Some(1));
    // The counts equal a recount of the rows.
    let connection = rusqlite::Connection::open(store.library_path()).unwrap();
    let recount: i64 = connection.query_row("SELECT COUNT(*) FROM assets WHERE type='audio'", [], |r| r.get(0)).unwrap();
    let stored: i64 = connection.query_row("SELECT n FROM facets WHERE type='audio' AND field='type'", [], |r| r.get(0)).unwrap();
    assert_eq!(recount, stored);
    let negative: i64 = connection.query_row("SELECT COUNT(*) FROM facets WHERE n <= 0", [], |r| r.get(0)).unwrap();
    assert_eq!(negative, 0, "no empty counts are kept");
}

#[test]
fn too_many_libraries_is_a_plain_sentence() {
    let tmp = Tmp::new("browse-limit");
    let libs = fixture(&tmp);
    let mut many = sources(&libs);
    for index in 0..40 {
        let mut copy = many[0].clone();
        copy.id = format!("x{index}");
        many.push(copy);
    }
    let error = Session::open(&many, &[]).err().expect("too many").to_string();
    assert!(error.contains("at most") && error.contains("libraries at once"), "{error}");
}

#[test]
fn personal_data_is_written_row_by_row() {
    let tmp = Tmp::new("browse-personal");
    let libs = fixture(&tmp);
    let store = &libs[0].store;
    let revision = store.status().unwrap().revision;
    let ids = |names: &[&str]| names.iter().map(|id| id.to_string()).collect::<Vec<_>>();
    let user_of = |id: &str| store.load_top_assets(&[id.to_string()]).unwrap().remove(0).user;

    store.set_favorite(&ids(&["a2", "a3"]), true).unwrap();
    assert!(user_of("a2").favorite && user_of("a3").favorite);
    assert!(user_of("a1").favorite, "other rows are left alone");
    store.set_favorite(&ids(&["a3"]), false).unwrap();
    assert!(!user_of("a3").favorite);
    // The list follows: favourites are a1, a4? (no), a2, s1 is another type.
    let mut f = filter("audio");
    f.favorites_only = true;
    f.libraries = vec!["a".into()];
    let favourites: Vec<String> = session(&libs, &f).page(&f, false, 0, 100).unwrap().into_iter().map(|(_, id)| id).collect();
    assert_eq!(favourites, ["a2", "a1"], "bang, Boom");

    store.set_collection_member(&ids(&["a2"]), "Fresh", true).unwrap();
    assert_eq!(user_of("a2").collections, ["Fresh"]);
    let session = Session::open(&sources(&libs), &[]).unwrap();
    assert!(session.collections("audio", &Visibility::default()).unwrap().iter().any(|c| c.name == "Fresh" && c.count == 1));
    store.set_collection_member(&ids(&["a2"]), "Fresh", false).unwrap();
    assert!(user_of("a2").collections.is_empty());
    assert_eq!(user_of("a1").collections, ["Zoo"]);

    store.set_preview_choice("a2", Some("a1")).unwrap();
    assert_eq!(user_of("a2").preview_asset_id.as_deref(), Some("a1"));
    store.set_preview_choice("a2", None).unwrap();
    assert_eq!(user_of("a2").preview_asset_id, None);

    store.drop_personal_tags(&[("a1".to_string(), vec!["mine".to_string()])]).unwrap();
    assert!(!user_of("a1").tags.contains(&"mine".to_string()));
    assert!(user_of("a1").tags.contains(&"source: Pack A".to_string()), "tags derived from the provenance stay");
    assert_eq!(store.status().unwrap().revision, revision, "personal data is not a library change");
}
