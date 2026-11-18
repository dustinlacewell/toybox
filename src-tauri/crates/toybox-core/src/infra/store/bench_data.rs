//! Synthetic rows for query benchmarks (`bin/bench-library.rs --query-bench`). Writes the rows of
//! `assets`, their tags and search rows, and some personal data straight into the databases, without
//! files or the import path (which takes about 13 minutes per 100,000 items), so a library of a
//! million items takes about a minute. The rows are what the import path would have written for audio and
//! plugin-type items: not meant for anything but measuring queries.

use std::path::Path;

use rusqlite::params;
use serde_json::json;

use super::{meta_set, open_or_create, Kind};
use crate::error::AppResult;

const WORDS: [&str; 40] = [
    "stone", "crate", "barrel", "wood", "metal", "rusty", "ancient", "modern", "forest", "desert", "castle", "village", "tower", "bridge", "window", "door", "table", "chair", "lamp", "bottle", "sword",
    "shield", "armor", "helmet", "banner", "torch", "chest", "rock", "tree", "bush", "grass", "water", "fire", "ice", "cloud", "star", "moon", "sun", "wind", "storm",
];
const CATEGORIES: [&str; 24] = [
    "props", "sfx", "ambient", "music", "voice", "weapons", "armor", "buildings", "nature", "vehicles", "furniture", "tools", "ui", "fx", "foliage", "terrain", "characters", "creatures", "decor", "signs", "lights",
    "doors", "windows", "misc",
];

/// What to generate.
pub struct Spec<'a> {
    /// Id prefix, so libraries do not share ids by accident.
    pub prefix: &'a str,
    pub count: usize,
    /// Distinct tags to draw from.
    pub tag_vocabulary: usize,
    /// Rows whose content hash equals that of a row of the library with this prefix and count
    /// (`every`th row, so a tenth of the rows when `every` is 10).
    pub copy_of: Option<(&'a str, usize, usize)>,
    /// A row in this many has the id of a row of the other library with different content.
    pub same_id_every: usize,
    /// A row in this many has no content hash yet.
    pub unhashed_every: usize,
}

fn lcg(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *state >> 33
}

/// The id and content hash of row `i` of a generated library with `prefix`.
fn hash_of(prefix: &str, i: usize) -> String {
    format!("{prefix}-{i:08x}")
}

/// The library-relative path of the file of generated row `i` (an audio row): the benchmark makes the ones it edits or exports.
pub fn item_file(i: usize) -> String {
    format!("library/pack{}/{}/x{i}.wav", i % 300, CATEGORIES[(i / 7) % 24])
}

pub fn generate(library_db: &Path, user_db: &Path, spec: &Spec) -> AppResult<()> {
    let mut library = open_or_create(library_db, Kind::Library)?;
    library.execute_batch("PRAGMA synchronous=OFF; PRAGMA cache_size=-200000;")?;
    let tx = library.transaction()?;
    let mut state = 0x9E3779B97F4A7C15u64 ^ spec.prefix.bytes().fold(0u64, |a, b| a.wrapping_mul(131).wrapping_add(b as u64));
    {
        let mut insert = tx.prepare(
            "INSERT INTO assets(id,item_id,ord,kind,type,name,pack,category,format,source,publisher,license,content_hash,hash_key,asset,revision,rel_path,thumb_name,clip_source,plugin) \
             VALUES(?1,?16,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,?13,1,?14,'', '', ?15)",
        )?;
        let mut item_insert = tx.prepare("INSERT INTO items(id,seq,kind,type,pack,category,name,storage_path,entry,revision,pkey) VALUES(?1,?2,?3,?4,?5,?6,?7,NULL,?8,1,?9)")?;
        let mut change_insert = tx.prepare("INSERT OR REPLACE INTO changes(revision,asset_id,deleted) VALUES(1,?1,0)")?;
        let mut tag_insert = tx.prepare("INSERT OR IGNORE INTO asset_tags(asset_id,tag,type) VALUES(?1,?2,?3)")?;
        let mut search = tx.prepare("INSERT INTO asset_search(rowid,name,tags) VALUES(?1,?2,?3)")?;
        for i in 0..spec.count {
            let mut id = format!("{}{i}", spec.prefix);
            let mut hash = Some(hash_of(spec.prefix, i));
            if let Some((other, count, every)) = spec.copy_of {
                if every > 0 && i % every == 0 && count > 0 {
                    hash = Some(hash_of(other, (i / every * 7919) % count));
                } else if spec.same_id_every > 0 && i % spec.same_id_every == 1 && count > 0 {
                    id = format!("{other}{}", (i * 31) % count);
                    hash = Some(format!("{}-v2", hash_of(spec.prefix, i)));
                }
            }
            if spec.unhashed_every > 0 && i % spec.unhashed_every == spec.unhashed_every - 1 {
                hash = None;
            }
            let (kind, type_id) = if i % 5 == 4 { ("plugin", "skybox") } else { ("audio", "audio") };
            let name = format!("{} {} {}", WORDS[i % 40], WORDS[(i / 40) % 40], i % 977);
            let category = CATEGORIES[(i / 7) % 24];
            let pack = format!("pack{}", i % 300);
            let mut tags: Vec<String> = Vec::new();
            for _ in 0..(3 + (lcg(&mut state) % 4)) {
                let u = (lcg(&mut state) % 1_000_000) as f64 / 1_000_000.0;
                let index = ((spec.tag_vocabulary as f64) * u * u * u) as usize;
                let tag = if index < 40 { WORDS[index].to_string() } else { format!("tag{index}") };
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
            let source = format!("Pack {}", i % 200);
            let mut blob = json!({
                "id": id, "name": name, "fileName": "x.wav", "relPath": format!("library/{pack}/{category}/x{i}.wav"), "pack": pack, "category": category,
                "tags": tags, "format": "wav", "provenance": {"source": source, "publisher": format!("Publisher {}", i % 50), "license": if i % 9 == 0 { "MIT" } else { "CC0" }},
            });
            if kind == "audio" {
                blob["kind"] = json!("audio");
                blob["audio"] = json!({"file": format!("library/{pack}/{category}/x{i}.wav"), "format": "wav", "duration": 1.0, "sampleRate": 48000, "channels": 2, "bitDepth": 16, "byteLength": 100, "modifiedNs": "1", "peaks": [0.1, 0.5, 0.9]});
            } else {
                blob["kind"] = json!("plugin");
                blob["type"] = json!("skybox");
                blob["files"] = json!([format!("library/{pack}/{category}/x{i}.hdr")]);
                blob["meta"] = json!({});
            }
            let rowid = {
                insert.execute(params![
                    id, i as i64, kind, type_id, name, pack, category, if kind == "audio" { "wav" } else { "hdr" }, source, format!("Publisher {}", i % 50),
                    if i % 9 == 0 { "MIT" } else { "CC0" }, hash, blob.to_string(), format!("library/{pack}/{category}/x{i}"), "audio-importer",
                    if kind == "audio" { Some(id.clone()) } else { None }
                ])?;
                if kind == "audio" {
                    // The manifest entry the item would have: what an edit or a delete of this row works on.
                    let entry = json!({"id": id, "kind": "audio", "pack": pack, "category": category, "file": format!("x{i}.wav"), "format": "wav", "tags": tags,
                        "provenance": {"source": source, "publisher": format!("Publisher {}", i % 50), "license": if i % 9 == 0 { "MIT" } else { "CC0" }}});
                    let columns = super::manifest::ItemColumns::of(&entry, &id);
                    item_insert.execute(params![id, i as i64, columns.kind, columns.type_id, columns.pack, columns.category, columns.name, entry.to_string(), columns.pkey])?;
                }
                tx.last_insert_rowid()
            };
            change_insert.execute(params![id])?;
            for tag in &tags {
                tag_insert.execute(params![id, tag, type_id])?;
            }
            search.execute(params![rowid, name, tags.join(" ")])?;
        }
    }
    meta_set(&tx, "revision", "1")?;
    meta_set(&tx, "hasManifest", "1")?;
    meta_set(&tx, "catalogSchema", &crate::config::SCHEMA_VERSION.to_string())?;
    // The rows are "derived for" the (empty) manifest as it is, so nothing rescans them away.
    meta_set(&tx, "manifestRevision", "1")?;
    meta_set(&tx, "derivedFor", "1")?;
    tx.commit()?;
    // The log of the load is as big as the library: empty it now, so a measurement does not pay for it.
    library.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    drop(library);

    // Personal data: a few favourites, ten collections of two hundred members, some personal tags.
    let mut user = open_or_create(user_db, Kind::User)?;
    let tx = user.transaction()?;
    {
        let step = (spec.count / 200).max(1);
        for k in 0..(spec.count / step).min(200) {
            tx.execute("INSERT OR IGNORE INTO user_assets(asset_id,favorite) VALUES(?1,1)", [format!("{}{}", spec.prefix, k * step)])?;
        }
        for collection in 0..10 {
            for k in 0..200.min(spec.count) {
                tx.execute(
                    "INSERT OR IGNORE INTO user_collections(asset_id,name) VALUES(?1,?2)",
                    params![format!("{}{}", spec.prefix, (k * 37 + collection * 11) % spec.count), format!("Collection {collection}")],
                )?;
            }
        }
        for k in 0..100.min(spec.count) {
            tx.execute("INSERT OR IGNORE INTO user_tags(asset_id,tag) VALUES(?1,'mine')", [format!("{}{}", spec.prefix, k * 13 % spec.count)])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Make a write the way a writer does: rename the assets `ids` (to `<name> <suffix>`), bump the revision once and record the
/// rows in `changes`. For benchmarks of what follows a change (a catch-up of the browse index, counts after a change, the feed).
pub fn touch_many(library_db: &Path, ids: &[String], suffix: &str) -> AppResult<()> {
    let mut library = open_or_create(library_db, Kind::Library)?;
    let tx = library.transaction()?;
    let revision: i64 = super::meta_int(&tx, "revision")? + 1;
    {
        let mut update = tx.prepare("UPDATE assets SET name = substr(name, 1, 40) || ?2, revision=?3 WHERE id=?1")?;
        let mut record = tx.prepare("INSERT OR REPLACE INTO changes(revision,asset_id,deleted) VALUES(?1,?2,0)")?;
        for id in ids {
            update.execute(params![id, format!(" {suffix}"), revision])?;
            record.execute(params![revision, id])?;
        }
    }
    meta_set(&tx, "revision", &revision.to_string())?;
    tx.commit()?;
    Ok(())
}
