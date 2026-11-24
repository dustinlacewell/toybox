//! What the window asks of the libraries: a page of the visible list, counts, ids.
//!
//! Several libraries are read together through one connection that `ATTACH`es each
//! `library.db` and `user.db` read-only (`l0`/`u0`, `l1`/`u1`, ...). Nothing here
//! holds a write lock and nothing loads a library: every query goes through an
//! index (the plans are checked in `browse_tests`).
//!
//! **The list.** One `SELECT` per library, merged by `ORDER BY name COLLATE NOCASE, id`
//! (the arms are already in that order through `assets_browse`, so the merge is a
//! streaming one) with `LIMIT`/`OFFSET`. When a filter has a small driving set
//! (a collection, favourites, a tag, a search of 3 characters or more) the arm starts
//! from that set instead, joined to `assets`.
//!
//! **Duplicates.** The same item in several libraries is shown once (the rules of
//! `src/domain/duplicates.ts`). A row of a library is hidden when a better-ranked
//! library (among the libraries in play) holds an item of the same type with the same
//! content hash, or, when either side has no hash yet, the same id. The test is a
//! `NOT EXISTS` probe on `assets_hash` (or the primary key), so the cost of a page is
//! the rows it passes over.
//!
//! **Counts.** `facets` (kept by triggers) holds the number of rows per type, category,
//! source, publisher, license, format and tag, so a count is a read of that table
//! minus the rows that are hidden (by a switched-off plugin or as a duplicate). The
//! hidden rows are found in one pass over the library, remembered per revision of the
//! libraries involved ([`TALLIES`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// A driving set bigger than this is not worth starting from: the browse index is walked instead.
const DRIVER_CAP: i64 = 20_000;
/// A search that matches more than this is not started from (its matches cost about 4 us each to read): the list is
/// walked by name instead.
const TEXT_CAP: i64 = 2_000;
/// Tags whose names look like provenance are the Sources, Publishers and Licenses groups.
const PROVENANCE_TAG: [&str; 3] = ["source:%", "publisher:%", "license:%"];

/// One library to read: where its databases are and how it ranks against the others
/// (lower wins when the same item is in two libraries).
#[derive(Debug, Clone)]
pub struct Source {
    pub id: String,
    pub library: PathBuf,
    pub user: PathBuf,
    pub rank: i64,
}

/// Which items are hidden because their plugin is switched off (see `pluginVisibility`
/// in `src/services/assetTypes.ts`, which makes these lists).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Visibility {
    /// Types that are hidden whatever imported them.
    pub hidden_types: Vec<String>,
    /// Plugins that are known and off: what they imported is hidden.
    pub plugins_off: Vec<String>,
    /// Plugins that are known and on: what they imported is shown (unless its type is hidden).
    pub plugins_on: Vec<String>,
    /// Retired plugin ids that a switched-on plugin replaced: shown.
    pub retired_on: Vec<String>,
    /// Retired plugin ids whose replacements are all off: hidden.
    pub retired_off: Vec<String>,
    /// Types whose owning plugin is off: hidden when no plugin decided otherwise.
    pub owner_off: Vec<String>,
}

/// Values that take an item out of the list, per field (the rail's "exclude" state). An item with any of them
/// is not listed, whatever else it matches.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Excluded {
    pub sources: Vec<String>,
    pub publishers: Vec<String>,
    pub licenses: Vec<String>,
    pub formats: Vec<String>,
    pub categories: Vec<String>,
    /// Library or personal tags.
    pub tags: Vec<String>,
}

impl Excluded {
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty() && self.publishers.is_empty() && self.licenses.is_empty() && self.formats.is_empty() && self.categories.is_empty() && self.tags.is_empty()
    }
}

/// A numeric property of the asset record within bounds. `path` is a dotted path into the record a type
/// declares (`audio.duration`, `meta.width`; see `docs/asset-types.md`); an item without the property is left out.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PropertyRange {
    pub path: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// The order of the list. `key`: `name` (or empty), `added`, `changed`, `publisher`, `source`, `license`,
/// `format`, `category`, `type`, or `prop:<path>` for a property a type declares. Ties are ordered by name.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct SortOrder {
    pub key: String,
    pub descending: bool,
}

/// The filter of the library view. Empty sets constrain nothing.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BrowseFilter {
    /// The asset type shown (`model`, `skybox`, ...).
    pub kind: String,
    /// Library ids; empty means every library.
    pub libraries: Vec<String>,
    pub sources: Vec<String>,
    pub publishers: Vec<String>,
    pub licenses: Vec<String>,
    pub formats: Vec<String>,
    pub categories: Vec<String>,
    /// Every one of these must be a tag of the item (library or personal).
    pub tags: Vec<String>,
    pub favorites_only: bool,
    pub collection: Option<String>,
    /// Case-insensitive substring of the name or of a tag.
    pub text: String,
    pub visibility: Visibility,
    /// Only the assets whose `origin.derivedFrom` is this raw (unqualified) id. With it, an empty `kind`
    /// means every type.
    pub derived_from: Option<String>,
    /// What takes an item out of the list.
    pub exclude: Excluded,
    /// With more than one tag: an item needs any of them instead of every one.
    pub tags_any: bool,
    /// Numeric properties, each within its bounds.
    pub ranges: Vec<PropertyRange>,
    pub sort: SortOrder,
}

impl BrowseFilter {
    /// Whether the filter looks at one type (`false`: derived assets of any type).
    fn typed(&self) -> bool {
        !(self.kind.is_empty() && self.derived_from.is_some())
    }

    /// Every chosen tag is required (the default), or there is at most one.
    fn tags_all(&self) -> bool {
        !self.tags_any || self.tags.len() <= 1
    }

    /// The list is in name order.
    fn by_name(&self) -> bool {
        matches!(self.sort.key.as_str(), "" | "name")
    }
}

/// `$.a.b` for a property path (`a.b`: words of letters, digits and `_`, not starting with a digit), else `None`.
pub fn json_path(path: &str) -> Option<String> {
    let word = |part: &str| {
        let mut chars = part.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    (path.len() <= 64 && path.split('.').all(word)).then(|| format!("$.{path}"))
}

/// What the list is ordered by before the name (`a` is the asset row), or `None` for name order. A property's
/// path goes into `p`; an unknown key orders by name.
fn sort_column(sort: &SortOrder, p: &mut Vec<Value>) -> Option<String> {
    match sort.key.as_str() {
        "" | "name" => None,
        "added" => Some("a.ord".into()),
        "changed" => Some("a.revision".into()),
        "publisher" | "source" | "license" | "format" | "category" => Some(format!("a.{} COLLATE NOCASE", sort.key)),
        key => key.strip_prefix("prop:").and_then(json_path).map(|path| {
            p.push(Value::Text(path));
            "json_extract(a.asset, ?)".to_string()
        }),
    }
}

/// A value and how many items have it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Count {
    pub value: String,
    pub count: i64,
}

/// The rail's numbers for one type (see `FacetFilter`): each group counts the items of the type,
/// every item once, whatever else is selected.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetCounts {
    /// Items of the type, each once.
    pub total: i64,
    /// Per library: what it holds of the type (every copy).
    pub libraries: Vec<Count>,
    pub categories: Vec<Count>,
    pub sources: Vec<Count>,
    pub publishers: Vec<Count>,
    pub licenses: Vec<Count>,
    pub formats: Vec<Count>,
    /// The most used tags and the selected ones.
    pub tags: Vec<Count>,
    /// How many different tags the type has.
    pub tag_total: i64,
}

/// A collection and how many items of the type are in it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionCount {
    pub name: String,
    pub count: i64,
}

/// A copy of an item in another library.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CopyIn {
    pub library: String,
    pub id: String,
}

/// Where else an item lives (`alsoIn`) and which copies are another version of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Elsewhere {
    pub library: String,
    pub id: String,
    pub also_in: Vec<CopyIn>,
    pub other_versions: Vec<CopyIn>,
}

// ---- the connection ---------------------------------------------------------------

struct Arm {
    id: String,
    /// Schema names of the attached databases.
    l: String,
    u: String,
}

/// The browse index attached to a session (see `browse_index`): which index position each arm has, and
/// whether the arms are every library the index holds (then duplicates can be left to its `sh` column).
struct IxUse {
    o: Vec<i64>,
    all: bool,
}

/// A read-only view over some libraries, best rank first.
pub struct Session {
    conn: Connection,
    arms: Vec<Arm>,
    ix: Option<IxUse>,
    ranks: Vec<i64>,
    /// Per arm: whether some item has no content hash yet.
    unhashed: Vec<bool>,
    drivers: RefCell<HashMap<String, Option<Rc<Driver>>>>,
    next_table: Cell<usize>,
}

pub(super) fn uri_for(path: &Path) -> String {
    uri(path)
}

fn uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_string();
    let mut out = String::from("file:");
    if !text.starts_with('/') {
        out.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'%' => out.push_str("%25"),
            b'?' => out.push_str("%3f"),
            b'#' => out.push_str("%23"),
            b' ' => out.push_str("%20"),
            other => out.push(other as char),
        }
    }
    out.push_str("?mode=ro");
    out
}

/// How many databases one connection may attach (`SQLITE_MAX_ATTACHED`, raised in `.cargo/config.toml`).
/// Each library takes two. A build that kept SQLite's default of 10 reports it when the attach fails.
const ATTACHED: usize = 40;

fn too_many(attached: usize) -> AppError {
    AppError::msg(format!("Toybox can show at most {} libraries at once. Remove a library or choose fewer in the Libraries filter.", attached / 2))
}

fn attach_error(error: &rusqlite::Error, path: &Path) -> AppError {
    let text = error.to_string();
    if text.contains("too many attached") {
        let max = text.rsplit(|c: char| !c.is_ascii_digit()).find(|part| !part.is_empty()).and_then(|part| part.parse().ok()).unwrap_or(10);
        return too_many(max);
    }
    AppError::msg(format!("Could not open the database {}: {text}", path.display()))
}

fn marks(count: usize) -> String {
    vec!["?"; count].join(",")
}

fn like_pattern(needle: &str) -> String {
    format!("%{}%", needle.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"))
}

fn text_value(text: &str) -> Value {
    Value::Text(text.to_string())
}

impl Session {
    /// Attach the libraries in `sources` (best rank first). With `only` non-empty, just those.
    /// A library whose database does not exist yet has nothing to show and is left out.
    pub fn open(sources: &[Source], only: &[String]) -> AppResult<Session> {
        Self::open_with(sources, only, None)
    }

    /// As [`Session::open`], using the browse index at `index` when it is current. A session that finds it behind brings it up to
    /// date first (a small catch-up here; a build that takes long on a background thread, while this session reads the libraries
    /// directly) and opens again.
    pub fn open_with(sources: &[Source], only: &[String], index: Option<&Path>) -> AppResult<Session> {
        let session = Self::open_once(sources, only, index)?;
        if let Some(path) = index {
            if session.ix.is_none() && matches!(super::browse_index::ensure(path, sources, false), Ok(super::browse_index::Ensured::Ready)) {
                return Self::open_once(sources, only, index);
            }
        }
        Ok(session)
    }

    fn open_once(sources: &[Source], only: &[String], index: Option<&Path>) -> AppResult<Session> {
        let trace = std::env::var("TOYBOX_TRACE_OPEN").is_ok();
        let began = std::time::Instant::now();
        let conn = Connection::open_in_memory_with_flags(OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        let mut chosen: Vec<&Source> = sources.iter().filter(|source| (only.is_empty() || only.contains(&source.id)) && source.library.is_file()).collect();
        chosen.sort_by_key(|source| source.rank);
        if chosen.len() * 2 > ATTACHED {
            return Err(too_many(ATTACHED));
        }
        let mut arms = Vec::new();
        let mut shown_ranks = Vec::new();
        for source in chosen.iter() {
            let index = arms.len();
            let (l, u) = (format!("l{index}"), format!("u{index}"));
            conn.execute(&format!("ATTACH DATABASE ?1 AS {l}"), [uri(&source.library)]).map_err(|e| attach_error(&e, &source.library))?;
            // A replica in the middle of its first download has no indexes yet and an incomplete list: it is not shown until it is complete.
            let loading: Option<i64> = conn.query_row(&format!("SELECT 1 FROM {l}.meta WHERE key = 'bulkLoad' AND value = '1'"), [], |row| row.get(0)).ok();
            if loading.is_some() {
                conn.execute_batch(&format!("DETACH DATABASE {l}"))?;
                continue;
            }
            shown_ranks.push(source.rank);
            if source.user.is_file() {
                conn.execute(&format!("ATTACH DATABASE ?1 AS {u}"), [uri(&source.user)]).map_err(|e| attach_error(&e, &source.user))?;
            } else {
                // Nothing personal yet: an empty stand-in with the same tables.
                conn.execute_batch(&format!(
                    "ATTACH DATABASE ':memory:' AS {u}; \
                     CREATE TABLE {u}.user_assets (asset_id TEXT PRIMARY KEY, favorite INTEGER NOT NULL DEFAULT 0, preview_asset TEXT, filesystem_path TEXT, thumb_error TEXT, provenance TEXT); \
                     CREATE TABLE {u}.user_tags (asset_id TEXT NOT NULL, tag TEXT NOT NULL, ord INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (asset_id, tag)) WITHOUT ROWID; \
                     CREATE TABLE {u}.user_collections (asset_id TEXT NOT NULL, name TEXT NOT NULL, ord INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (asset_id, name)) WITHOUT ROWID;"
                ))?;
            }
            arms.push(Arm { id: source.id.clone(), l, u });
        }
        if trace {
            eprintln!("  open: attached {:.1} ms", began.elapsed().as_secs_f64() * 1000.0);
        }
        let mut ix = None;
        if let Some(path) = index {
            if let Ok(found) = Self::attach_index(&conn, path, sources, &arms) {
                ix = found;
            }
        }
        conn.execute_batch("BEGIN")?;
        let mut unhashed = Vec::new();
        for arm in &arms {
            let found: Option<i64> = conn.query_row(&format!("SELECT 1 FROM {}.assets INDEXED BY assets_unhashed WHERE content_hash IS NULL LIMIT 1", arm.l), [], |row| row.get(0)).ok();
            unhashed.push(found.is_some());
        }
        if trace {
            eprintln!("  open: done {:.1} ms", began.elapsed().as_secs_f64() * 1000.0);
        }
        Ok(Session { conn, arms, ix, ranks: shown_ranks, unhashed, drivers: RefCell::new(HashMap::new()), next_table: Cell::new(0) })
    }

    /// Attach the index read-only if it describes the libraries as they are (same libraries in the same order, same
    /// database, same revision for every arm). `None` otherwise: the session reads the libraries directly.
    fn attach_index(conn: &Connection, path: &Path, sources: &[Source], arms: &[Arm]) -> AppResult<Option<IxUse>> {
        if !path.is_file() {
            return Ok(None);
        }
        conn.execute("ATTACH DATABASE ?1 AS ix", [uri(path)])?;
        let all = super::browse_index::indexed(sources);
        let libs = super::browse_index::read_libs(conn, "ix")?;
        if libs.len() != all.len() || libs.iter().zip(&all).any(|(have, source)| have.id != source.id) {
            return Ok(None);
        }
        let mut o = Vec::new();
        for arm in arms {
            let Some(have) = libs.iter().find(|lib| lib.id == arm.id) else { return Ok(None) };
            let mut db_id = String::new();
            let mut revision = 0i64;
            let mut statement = conn.prepare(&format!("SELECT key, value FROM {}.meta WHERE key IN ('dbId','revision')", arm.l))?;
            for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))? {
                let (key, value) = row?;
                if key == "dbId" {
                    db_id = value;
                } else {
                    revision = value.parse().unwrap_or(0);
                }
            }
            if have.db_id != db_id || have.revision != revision {
                return Ok(None);
            }
            o.push(have.pos);
        }
        Ok(Some(IxUse { all: arms.len() == libs.len(), o }))
    }

    /// Whether the index is attached and every arm of this session is current in it.
    pub fn has_index(&self) -> bool {
        self.ix.is_some()
    }

    /// The list of a type with no other constraint: what the index serves directly (every library, or one).
    fn plain(&self, f: &BrowseFilter) -> bool {
        self.ix.as_ref().is_some_and(|ix| ix.all || self.arms.len() == 1)
            && f.derived_from.is_none()
            && f.by_name()
            && f.exclude.is_empty()
            && f.ranges.is_empty()
            && f.text.trim().is_empty()
            && f.tags.is_empty()
            && !f.favorites_only
            && f.collection.is_none()
            && f.sources.is_empty()
            && f.publishers.is_empty()
            && f.licenses.is_empty()
            && f.formats.is_empty()
            && f.categories.is_empty()
            && Self::visible(&f.visibility, "a", &mut Vec::new()).is_none()
    }

    pub fn library_ids(&self) -> Vec<String> {
        self.arms.iter().map(|arm| arm.id.clone()).collect()
    }

    fn arm_index(&self, library: &str) -> Option<usize> {
        self.arms.iter().position(|arm| arm.id == library)
    }

    /// `revision`/`hashRevision` of every library: what a remembered count depends on.
    fn stamp(&self) -> AppResult<String> {
        let mut out = String::new();
        for (arm, rank) in self.arms.iter().zip(&self.ranks) {
            let mut statement = self.conn.prepare(&format!("SELECT key, value FROM {}.meta WHERE key IN ('revision','hashRevision','dbId')", arm.l))?;
            let mut found: Vec<(String, String)> = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
            found.sort();
            out.push_str(&format!("{}@{rank}:{found:?};", arm.id));
        }
        Ok(out)
    }

    // ---- SQL pieces ---------------------------------------------------------------

    /// `a` is shown (its plugin is on). `None` when nothing is hidden.
    fn visible(visibility: &Visibility, a: &str, p: &mut Vec<Value>) -> Option<String> {
        let list = |column: &str, values: &[String], negate: bool, p: &mut Vec<Value>| {
            p.extend(values.iter().map(|value| text_value(value)));
            format!("{a}.{column} {}IN ({})", if negate { "NOT " } else { "" }, marks(values.len()))
        };
        let mut hidden: Vec<String> = Vec::new();
        if !visibility.hidden_types.is_empty() {
            hidden.push(list("type", &visibility.hidden_types, false, p));
        }
        if !visibility.plugins_off.is_empty() {
            hidden.push(list("plugin", &visibility.plugins_off, false, p));
        }
        if !visibility.retired_off.is_empty() || !visibility.owner_off.is_empty() {
            let mut guard: Vec<String> = Vec::new();
            if !visibility.plugins_on.is_empty() {
                guard.push(list("plugin", &visibility.plugins_on, true, p));
            }
            if !visibility.retired_on.is_empty() {
                guard.push(list("plugin", &visibility.retired_on, true, p));
            }
            let mut undecided: Vec<String> = Vec::new();
            if !visibility.retired_off.is_empty() {
                undecided.push(list("plugin", &visibility.retired_off, false, p));
            }
            if !visibility.owner_off.is_empty() {
                undecided.push(list("type", &visibility.owner_off, false, p));
            }
            guard.push(format!("({})", undecided.join(" OR ")));
            hidden.push(format!("({})", guard.join(" AND ")));
        }
        if hidden.is_empty() { None } else { Some(format!("NOT ({})", hidden.join(" OR "))) }
    }

    /// `a` of arm `arm` has no counterpart in a library that ranks before it. With `kind`, only items of that type
    /// are looked at. Two terms per better library: the hash probe (one index lookup), and, only when either
    /// library has items without a hash yet, the set of rows that match such an item by id (found once per query
    /// from the small partial index `assets_unhashed`).
    fn first_copy(&self, arm: usize, a: &str, visibility: &Visibility, kind: Option<&str>, p: &mut Vec<Value>) -> Option<String> {
        if arm == 0 {
            return None;
        }
        let mut parts = Vec::new();
        for (j, better) in self.arms[..arm].iter().enumerate() {
            let mut by_hash = format!("NOT EXISTS (SELECT 1 FROM {}.assets b INDEXED BY assets_hash WHERE b.type = {a}.type AND b.content_hash = {a}.content_hash", better.l);
            if let Some(shown) = Self::visible(visibility, "b", p) {
                by_hash.push_str(&format!(" AND {shown}"));
            }
            by_hash.push(')');
            parts.push(by_hash);
            if self.unhashed[arm] || self.unhashed[j] {
                let (li, lj) = (&self.arms[arm].l, &better.l);
                let mut sets = Vec::new();
                // Unhashed items of the better library, and the rows of this one that share their id.
                let mut first = format!("SELECT x.rowid FROM {lj}.assets b INDEXED BY assets_unhashed CROSS JOIN {li}.assets x INDEXED BY sqlite_autoindex_assets_1 ON x.id = b.id AND x.type = b.type WHERE b.content_hash IS NULL");
                if let Some(kind) = kind {
                    first.push_str(" AND b.type = ?");
                    p.push(text_value(kind));
                }
                if let Some(shown) = Self::visible(visibility, "b", p) {
                    first.push_str(&format!(" AND {shown}"));
                }
                sets.push(first);
                // Unhashed rows of this library that share an id with any item of the better one.
                let mut second = format!("SELECT x.rowid FROM {li}.assets x INDEXED BY assets_unhashed CROSS JOIN {lj}.assets b INDEXED BY sqlite_autoindex_assets_1 ON b.id = x.id AND b.type = x.type WHERE x.content_hash IS NULL");
                if let Some(kind) = kind {
                    second.push_str(" AND x.type = ?");
                    p.push(text_value(kind));
                }
                if let Some(shown) = Self::visible(visibility, "b", p) {
                    second.push_str(&format!(" AND {shown}"));
                }
                sets.push(second);
                parts.push(format!("{a}.rowid NOT IN ({})", sets.join(" UNION ")));
            }
        }
        Some(parts.join(" AND "))
    }

    /// How many rows the personal or library data holds for one condition, stopping once it is past `DRIVER_CAP`.
    fn bounded_count(&self, sql: &str, p: &[Value]) -> AppResult<i64> {
        Ok(self.conn.query_row(&format!("SELECT COUNT(*) FROM (SELECT 1 FROM {sql} LIMIT {})", DRIVER_CAP + 1), params_from_iter(p.iter()), |row| row.get(0))?)
    }

    /// The largest driving set a page starts from in an arm: sorting the set costs about 3 us a row, walking the list
    /// for the first 100 matches costs about 100 / density us, which balances at about 6 * sqrt(rows).
    fn page_cap(&self, arm: usize, kind: &str) -> i64 {
        let rows: i64 = self.conn.query_row(&format!("SELECT COALESCE(SUM(n), 0) FROM {}.facets WHERE type = ? AND field = 'type'", self.arms[arm].l), [kind], |row| row.get(0)).unwrap_or(0);
        ((6.0 * (rows as f64).sqrt()) as i64).max(2_000)
    }

    /// The driving set of an arm, if the filter has a small one: the smallest of a collection, favourites, a
    /// source/publisher/license/format, a tag and a search of 3 characters or more, estimated from the counts
    /// the library keeps (or, for a search, by running it). It is copied into a temporary table once, so the
    /// page and the count of one request share it.
    fn driver(&self, arm: usize, f: &BrowseFilter) -> AppResult<Option<Rc<Driver>>> {
        let key = format!("{arm}|{f:?}");
        if let Some(found) = self.drivers.borrow().get(&key) {
            return Ok(found.clone());
        }
        let (l, u) = (&self.arms[arm].l, &self.arms[arm].u);
        let mut best: Option<(i64, What, String, Vec<Value>, bool)> = None; // (size, what, select, params, by_rowid)
        let mut consider = |size: i64, what: What, select: String, params: Vec<Value>, by_rowid: bool| {
            if size <= DRIVER_CAP && best.as_ref().is_none_or(|(smallest, ..)| size < *smallest) {
                best = Some((size, what, select, params, by_rowid));
            }
        };
        if let Some(name) = &f.collection {
            let p = vec![text_value(name)];
            let size = self.bounded_count(&format!("{u}.user_collections WHERE name = ?"), &p)?;
            consider(size, What::Collection, format!("SELECT asset_id FROM {u}.user_collections WHERE name = ?"), p, false);
        }
        if f.favorites_only {
            let size = self.bounded_count(&format!("{u}.user_assets WHERE favorite = 1"), &[])?;
            consider(size, What::Favorites, format!("SELECT asset_id FROM {u}.user_assets WHERE favorite = 1"), vec![], false);
        }
        for (field, values) in [("source", &f.sources), ("publisher", &f.publishers), ("license", &f.licenses), ("format", &f.formats)] {
            if values.is_empty() {
                continue;
            }
            let mut p = vec![text_value(&f.kind), text_value(field)];
            p.extend(values.iter().map(|value| text_value(value)));
            let size: i64 = self.conn.query_row(&format!("SELECT COALESCE(SUM(n), 0) FROM {l}.facets WHERE type = ? AND field = ? AND value IN ({})", marks(values.len())), params_from_iter(p.iter()), |row| row.get(0))?;
            let mut q = vec![text_value(&f.kind)];
            q.extend(values.iter().map(|value| text_value(value)));
            consider(size, What::Field(field), format!("SELECT rowid FROM {l}.assets WHERE type = ? AND {field} IN ({})", marks(values.len())), q, true);
        }
        if let (Some(tag), true) = (f.tags.first(), f.tags_all()) {
            let library: i64 = self.conn.query_row(&format!("SELECT COALESCE(SUM(n), 0) FROM {l}.facets WHERE type = ? AND field = 'tag' AND value = ?"), params_from_iter([text_value(&f.kind), text_value(tag)].iter()), |row| row.get(0))?;
            let personal = self.bounded_count(&format!("{u}.user_tags WHERE tag = ?"), &[text_value(tag)])?;
            consider(
                library + personal,
                What::Tag,
                format!("SELECT asset_id FROM {l}.asset_tags WHERE tag = ? UNION SELECT asset_id FROM {u}.user_tags WHERE tag = ?"),
                vec![text_value(tag), text_value(tag)],
                false,
            );
        }
        let needle = f.text.trim().to_lowercase();
        let mut chosen = best.map(|(size, what, select, params, by_rowid)| (size, what, select, params, by_rowid));
        if needle.chars().count() >= 3 {
            let select = format!(
                "SELECT rowid FROM {l}.asset_search WHERE asset_search MATCH ? \
                 UNION SELECT a2.rowid FROM {u}.user_tags t CROSS JOIN {l}.assets a2 ON a2.id = t.asset_id WHERE t.tag LIKE ? ESCAPE '\\'"
            );
            let params = vec![text_value(&fts_phrase(&needle)), text_value(&like_pattern(&needle))];
            // A search is only sized by running it; stop at the cap.
            let table = format!("drv{}", self.next_table.replace(self.next_table.get() + 1));
            self.conn.execute_batch(&format!("CREATE TEMP TABLE {table} (k INTEGER PRIMARY KEY)"))?;
            self.conn.execute(&format!("INSERT OR IGNORE INTO {table} SELECT * FROM ({select}) LIMIT {}", TEXT_CAP + 1), params_from_iter(params.iter()))?;
            let size: i64 = self.conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))?;
            if size <= TEXT_CAP && chosen.as_ref().is_none_or(|(smallest, ..)| size < *smallest) {
                let driver = Rc::new(Driver { table, by_rowid: true, what: What::Text, size });
                self.drivers.borrow_mut().insert(key, Some(driver.clone()));
                return Ok(Some(driver));
            }
            self.conn.execute_batch(&format!("DROP TABLE {table}"))?;
        }
        let Some((chosen_size, what, select, params, by_rowid)) = chosen.take() else {
            self.drivers.borrow_mut().insert(key, None);
            return Ok(None);
        };
        let table = format!("drv{}", self.next_table.replace(self.next_table.get() + 1));
        self.conn.execute_batch(&format!("CREATE TEMP TABLE {table} (k {} PRIMARY KEY) {}", if by_rowid { "INTEGER" } else { "TEXT" }, if by_rowid { "" } else { "WITHOUT ROWID" }))?;
        self.conn.execute(&format!("INSERT OR IGNORE INTO {table} {select}"), params_from_iter(params.iter()))?;
        let driver = Rc::new(Driver { table, by_rowid, what, size: chosen_size });
        self.drivers.borrow_mut().insert(key, Some(driver.clone()));
        Ok(Some(driver))
    }

    /// `FROM ... WHERE ...` of arm `arm` for `f`, and its parameters.
    /// With `page`, a driving set bigger than a page is worth is not used: the list is walked in order instead, which finds a
    /// page of matches after reading about `100 / density` rows, where starting from the set means sorting all of it.
    fn arm_from(&self, arm: usize, f: &BrowseFilter, with_type: bool, page: bool) -> AppResult<(String, Vec<Value>)> {
        let (l, u) = (&self.arms[arm].l, &self.arms[arm].u);
        let mut p: Vec<Value> = Vec::new();
        let driver = self.driver(arm, f)?.filter(|driver| !page || driver.size <= self.page_cap(arm, &f.kind));
        let mut from = match &driver {
            Some(driver) => format!("FROM temp.{} d CROSS JOIN {l}.assets a ON a.{} = d.k", driver.table, if driver.by_rowid { "rowid" } else { "id" }),
            None => format!("FROM {l}.assets a"),
        };
        let driven = driver.as_ref().map(|d| d.what);
        let mut conds: Vec<String> = Vec::new();
        let with_type = with_type && f.typed();
        if with_type {
            conds.push("a.type = ?".into());
            p.push(text_value(&f.kind));
        }
        if let Some(parent) = &f.derived_from {
            conds.push("a.derived_from = ?".into());
            p.push(text_value(parent));
        }
        if let Some(shown) = Self::visible(&f.visibility, "a", &mut p) {
            conds.push(shown);
        }
        for (column, values) in [("source", &f.sources), ("publisher", &f.publishers), ("license", &f.licenses), ("format", &f.formats), ("category", &f.categories)] {
            if !values.is_empty() && driven != Some(What::Field(column)) {
                conds.push(format!("a.{column} IN ({})", marks(values.len())));
                p.extend(values.iter().map(|value| text_value(value)));
            }
        }
        let has_tag = |values: &[String], p: &mut Vec<Value>| {
            p.extend(values.iter().map(|value| text_value(value)));
            p.extend(values.iter().map(|value| text_value(value)));
            format!(
                "(EXISTS (SELECT 1 FROM {l}.asset_tags t WHERE t.asset_id = a.id AND t.tag IN ({m})) OR EXISTS (SELECT 1 FROM {u}.user_tags t WHERE t.asset_id = a.id AND t.tag IN ({m})))",
                m = marks(values.len())
            )
        };
        if f.tags_all() {
            for (index, tag) in f.tags.iter().enumerate() {
                if index == 0 && driven == Some(What::Tag) {
                    continue;
                }
                conds.push(has_tag(std::slice::from_ref(tag), &mut p));
            }
        } else {
            conds.push(has_tag(&f.tags, &mut p));
        }
        for (column, values) in [("source", &f.exclude.sources), ("publisher", &f.exclude.publishers), ("license", &f.exclude.licenses), ("format", &f.exclude.formats), ("category", &f.exclude.categories)] {
            if !values.is_empty() {
                conds.push(format!("a.{column} NOT IN ({})", marks(values.len())));
                p.extend(values.iter().map(|value| text_value(value)));
            }
        }
        if !f.exclude.tags.is_empty() {
            let tagged = has_tag(&f.exclude.tags, &mut p);
            conds.push(format!("NOT {tagged}"));
        }
        for range in &f.ranges {
            let path = json_path(&range.path).ok_or_else(|| AppError::msg(format!("'{}' is not a property path", range.path)))?;
            // Only a number is in range: SQLite orders any text above every number.
            conds.push("json_type(a.asset, ?) IN ('integer', 'real')".into());
            p.push(Value::Text(path.clone()));
            if let Some(min) = range.min {
                conds.push("json_extract(a.asset, ?) >= ?".into());
                p.extend([Value::Text(path.clone()), Value::Real(min)]);
            }
            if let Some(max) = range.max {
                conds.push("json_extract(a.asset, ?) <= ?".into());
                p.extend([Value::Text(path), Value::Real(max)]);
            }
        }
        if f.favorites_only && driven != Some(What::Favorites) {
            conds.push(format!("EXISTS (SELECT 1 FROM {u}.user_assets x WHERE x.asset_id = a.id AND x.favorite = 1)"));
        }
        if let (Some(name), true) = (&f.collection, driven != Some(What::Collection)) {
            conds.push(format!("EXISTS (SELECT 1 FROM {u}.user_collections x WHERE x.asset_id = a.id AND x.name = ?)"));
            p.push(text_value(name));
        }
        let needle = f.text.trim().to_lowercase();
        if !needle.is_empty() && driven != Some(What::Text) {
            {
                // A search too short for the search index, or one that matches a great many items (so starting from
                // its matches would cost more than walking the list), looks at the names as the list is walked. The
                // tags that contain it are found among the tags the library counts (a short list), and their
                // items through the tag index; a needle that matches very many tags scans the tag index instead.
                let like = text_value(&like_pattern(&needle));
                let (matching, tagged_rows): (Vec<String>, i64) = {
                    let mut statement = self.conn.prepare(&format!("SELECT value, n FROM {l}.facets WHERE type = ? AND field = 'tag' AND value LIKE ? ESCAPE '\\' LIMIT 301"))?;
                    let found: Vec<(String, i64)> = statement.query_map(params_from_iter([text_value(&f.kind), like.clone()].iter()), |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
                    (found.iter().map(|(tag, _)| tag.clone()).collect(), found.iter().map(|(_, n)| n).sum())
                };
                if matching.len() <= 300 {
                    // Counting builds the set of items with such a tag once; a page walks the list, and a tag that is on many
                    // items is then looked for in each row's own few tags instead.
                    let by_tag = if matching.is_empty() {
                        "0".to_string()
                    } else if !page || tagged_rows <= DRIVER_CAP / 4 {
                        format!("a.id IN (SELECT asset_id FROM {l}.asset_tags WHERE tag IN ({}))", marks(matching.len()))
                    } else {
                        format!("EXISTS (SELECT 1 FROM {l}.asset_tags t WHERE t.asset_id = a.id AND t.tag IN ({}))", marks(matching.len()))
                    };
                    conds.push(format!("(a.name LIKE ? ESCAPE '\\' OR {by_tag} OR a.id IN (SELECT asset_id FROM {u}.user_tags WHERE tag LIKE ? ESCAPE '\\'))"));
                    p.push(like.clone());
                    p.extend(matching.iter().map(|tag| text_value(tag)));
                    p.push(like);
                } else {
                    // Many items carry such a tag: each row looks at its own few tags instead of building the set.
                    conds.push(format!(
                        "(a.name LIKE ? ESCAPE '\\' OR EXISTS (SELECT 1 FROM {l}.asset_tags t WHERE t.asset_id = a.id AND t.tag LIKE ? ESCAPE '\\') OR a.id IN (SELECT asset_id FROM {u}.user_tags WHERE tag LIKE ? ESCAPE '\\'))"
                    ));
                    p.extend([like.clone(), like.clone(), like]);
                }
            }
        }
        if let Some(first) = self.first_copy(arm, "a", &f.visibility, with_type.then_some(f.kind.as_str()), &mut p) {
            conds.push(first);
        }
        if !conds.is_empty() {
            from.push_str(" WHERE ");
            from.push_str(&conds.join(" AND "));
        }
        Ok((from, p))
    }

    // ---- the list -----------------------------------------------------------------

    /// How many items `f` shows.
    pub fn count(&self, f: &BrowseFilter) -> AppResult<i64> {
        if let Some(total) = self.counted(f)? {
            return Ok(total);
        }
        let mut total = 0;
        for arm in 0..self.arms.len() {
            let (from, p) = self.arm_from(arm, f, true, false)?;
            total += self.conn.query_row(&format!("SELECT COUNT(*) {from}"), params_from_iter(p.iter()), |row| row.get::<_, i64>(0))?;
        }
        Ok(total)
    }

    /// The count when it does not need a pass over the list: a filter the libraries' counts answer, or one whose every
    /// arm starts from a small driving set, or one with no search text. `None` for a broad search (the exact number
    /// is `count`, asked for separately so the first page does not wait for it).
    pub fn count_cheap(&self, f: &BrowseFilter) -> AppResult<Option<i64>> {
        if let Some(total) = self.counted(f)? {
            return Ok(Some(total));
        }
        if !f.text.trim().is_empty() {
            for arm in 0..self.arms.len() {
                let driven = self.driver(arm, f)?.is_some();
                if !driven {
                    return Ok(None);
                }
            }
        }
        Ok(Some(self.count(f)?))
    }

    /// The count of a filter with at most one constraint (a type; one tag; one group of categories, sources,
    /// publishers, licenses or formats), read from the counts the libraries keep minus the hidden rows
    /// (remembered per revision), so it does not walk the list. `None` for any other filter.
    fn counted(&self, f: &BrowseFilter) -> AppResult<Option<i64>> {
        if !f.text.trim().is_empty() || f.favorites_only || f.collection.is_some() || f.tags.len() > 1 || !f.exclude.is_empty() || !f.ranges.is_empty() || f.derived_from.is_some() {
            return Ok(None);
        }
        let groups: Vec<(&str, &Vec<String>)> = [("source", &f.sources), ("publisher", &f.publishers), ("license", &f.licenses), ("format", &f.formats), ("category", &f.categories)]
            .into_iter()
            .filter(|(_, values)| !values.is_empty())
            .collect();
        if groups.len() + f.tags.len() > 1 {
            return Ok(None);
        }
        let hidden = self.hidden(Some(&f.kind), &f.visibility)?;
        let mut total = 0;
        if let Some(tag) = f.tags.first() {
            for arm in &self.arms {
                total += self.conn.query_row(&format!("SELECT COALESCE(SUM(n), 0) FROM {}.facets WHERE type = ? AND field = 'tag' AND value = ?", arm.l), params_from_iter([text_value(&f.kind), text_value(tag)].iter()), |row| row.get::<_, i64>(0))?;
                total += self.conn.query_row(
                    &format!(
                        "SELECT COUNT(*) FROM {u}.user_tags t CROSS JOIN {l}.assets a ON a.id = t.asset_id WHERE t.tag = ?1 AND a.type = ?2 AND NOT EXISTS (SELECT 1 FROM {l}.asset_tags x WHERE x.asset_id = t.asset_id AND x.tag = t.tag)",
                        u = arm.u,
                        l = arm.l
                    ),
                    params_from_iter([text_value(tag), text_value(&f.kind)].iter()),
                    |row| row.get::<_, i64>(0),
                )?;
            }
            total -= hidden.fields.get(&("tag".to_string(), tag.clone())).copied().unwrap_or(0);
        } else if let Some((field, values)) = groups.first() {
            for arm in &self.arms {
                let mut p = vec![text_value(&f.kind), text_value(field)];
                p.extend(values.iter().map(|value| text_value(value)));
                total += self.conn.query_row(&format!("SELECT COALESCE(SUM(n), 0) FROM {}.facets WHERE type = ? AND field = ? AND value IN ({})", arm.l, marks(values.len())), params_from_iter(p.iter()), |row| row.get::<_, i64>(0))?;
            }
            for value in values.iter() {
                total -= hidden.fields.get(&(field.to_string(), value.clone())).copied().unwrap_or(0);
            }
        } else {
            for arm in &self.arms {
                total += self.conn.query_row(&format!("SELECT COALESCE(SUM(n), 0) FROM {}.facets WHERE type = ? AND field = 'type'", arm.l), [&f.kind], |row| row.get::<_, i64>(0))?;
            }
            total -= hidden.fields.get(&("type".to_string(), f.kind.clone())).copied().unwrap_or(0);
        }
        Ok(Some(total.max(0)))
    }

    /// `limit` items of the list starting at `offset`, as `(library id, asset id)`.
    pub fn page(&self, f: &BrowseFilter, descending: bool, offset: i64, limit: i64) -> AppResult<Vec<(String, String)>> {
        if self.arms.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let descending = descending != f.sort.descending;
        if self.plain(f) {
            return self.page_from_index(f, descending, offset, limit);
        }
        let mut sql = String::new();
        let mut p: Vec<Value> = Vec::new();
        let mut sorted = false;
        for arm in 0..self.arms.len() {
            let (from, arm_params) = self.arm_from(arm, f, true, true)?;
            if arm > 0 {
                sql.push_str(" UNION ALL ");
            }
            // A sort key comes first; items without it go last whichever way the list runs.
            // (A compound query orders by its columns only, so "has no value" is a column of its own; the key's
            // parameter, if any, is the same for both.)
            let key = sort_column(&f.sort, &mut p).map(|column| {
                sorted = true;
                let _ = sort_column(&f.sort, &mut p);
                format!(", {column} AS s, ({column}) IS NULL AS z")
            });
            sql.push_str(&format!("SELECT a.name COLLATE NOCASE AS n, a.id AS i, {arm} AS o{} {from}", key.unwrap_or_default()));
            p.extend(arm_params);
        }
        let dir = if descending { " DESC" } else { "" };
        let order = if sorted { format!("z, s{dir}, n, i, o") } else { format!("n{dir}, i{dir}, o{dir}") };
        sql.push_str(&format!(" ORDER BY {order} LIMIT ? OFFSET ?"));
        p.push(Value::Integer(limit));
        p.push(Value::Integer(offset.max(0)));
        let mut statement = self.conn.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(p.iter()))?;
        let mut out = Vec::with_capacity(limit as usize);
        while let Some(row) = rows.next()? {
            let arm: i64 = row.get(2)?;
            out.push((self.arms[arm as usize].id.clone(), row.get::<_, String>(1)?));
        }
        Ok(out)
    }

    /// The page of an unconstrained list from the index: one walk of `merged` in key order, copies left out by `sh`.
    fn page_from_index(&self, f: &BrowseFilter, descending: bool, offset: i64, limit: i64) -> AppResult<Vec<(String, String)>> {
        let ix = self.ix.as_ref().expect("plain() checked");
        let single = !ix.all;
        let order = if descending { "name DESC, id DESC, o DESC" } else { "name, id, o" };
        let sql = format!("SELECT id, o FROM ix.merged WHERE type = ?1 AND {} ORDER BY {order} LIMIT ?2 OFFSET ?3", if single { "o = ?4" } else { "sh = 0 AND ?4 = ?4" });
        let mut statement = self.conn.prepare_cached(&sql)?;
        let mut rows = statement.query(rusqlite::params![f.kind, limit, offset.max(0), ix.o.first().copied().unwrap_or(0)])?;
        let mut out = Vec::with_capacity(limit as usize);
        while let Some(row) = rows.next()? {
            let o: i64 = row.get(1)?;
            let arm = if single { 0 } else { o as usize };
            out.push((self.arms[arm].id.clone(), row.get::<_, String>(0)?));
        }
        Ok(out)
    }

    /// The EXPLAIN QUERY PLAN of the index page (tests assert on it).
    pub fn explain_index_page(&self, f: &BrowseFilter, offset: i64, limit: i64) -> AppResult<Vec<String>> {
        let mut statement = self.conn.prepare("EXPLAIN QUERY PLAN SELECT id, o FROM ix.merged WHERE type = ?1 AND sh = 0 AND ?4 = ?4 ORDER BY name, id, o LIMIT ?2 OFFSET ?3")?;
        let rows = statement.query_map(rusqlite::params![f.kind, limit, offset, 0], |row| row.get::<_, String>(3))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The EXPLAIN QUERY PLAN of the list query (tests assert on it).
    pub fn explain_page(&self, f: &BrowseFilter, offset: i64, limit: i64) -> AppResult<Vec<String>> {
        let mut sql = String::from("EXPLAIN QUERY PLAN ");
        let mut p: Vec<Value> = Vec::new();
        for arm in 0..self.arms.len() {
            let (from, arm_params) = self.arm_from(arm, f, true, true)?;
            if arm > 0 {
                sql.push_str(" UNION ALL ");
            }
            sql.push_str(&format!("SELECT a.name COLLATE NOCASE AS n, a.id AS i, {arm} AS o {from}"));
            p.extend(arm_params);
        }
        sql.push_str(" ORDER BY n, i, o LIMIT ? OFFSET ?");
        p.push(Value::Integer(limit));
        p.push(Value::Integer(offset));
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(p.iter()), |row| row.get::<_, String>(3))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ---- counts -------------------------------------------------------------------

    /// Items hidden from a count: rows of a hidden plugin and rows that duplicate a better library's.
    fn hidden(&self, kind: Option<&str>, visibility: &Visibility) -> AppResult<Arc<Tallies>> {
        let key = format!("{}|{}|{}", self.stamp()?, kind.unwrap_or("*"), serde_json::to_string(visibility).unwrap_or_default());
        if let Some(found) = TALLIES.lock().unwrap_or_else(|p| p.into_inner()).get(&key) {
            return Ok(found.clone());
        }
        if let Some(ix) = &self.ix {
            if ix.all && Self::visible(visibility, "a", &mut Vec::new()).is_none() {
                return Ok(Arc::new(self.hidden_from_index(kind)?));
            }
        }
        self.conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS hid (arm INTEGER NOT NULL, id TEXT NOT NULL, PRIMARY KEY (arm, id)) WITHOUT ROWID; DELETE FROM hid;")?;
        let mut tallies = Tallies::default();
        for (arm, entry) in self.arms.iter().enumerate() {
            let mut p: Vec<Value> = Vec::new();
            let mut shown: Vec<String> = Vec::new();
            if let Some(visible) = Self::visible(visibility, "a", &mut p) {
                shown.push(visible);
            }
            if let Some(first) = self.first_copy(arm, "a", visibility, kind, &mut p) {
                shown.push(first);
            }
            if shown.is_empty() {
                continue;
            }
            let mut sql = format!("INSERT OR IGNORE INTO hid SELECT {arm}, a.id FROM {}.assets a WHERE NOT ({})", entry.l, shown.join(" AND "));
            if let Some(kind) = kind {
                sql.push_str(" AND a.type = ?");
                p.push(text_value(kind));
            }
            self.conn.execute(&sql, params_from_iter(p.iter()))?;
            let fields: &[&str] = if kind.is_some() { &["type", "category", "source", "publisher", "license", "format"] } else { &["type"] };
            for field in fields {
                let mut statement = self.conn.prepare(&format!("SELECT a.{field}, COUNT(*) FROM hid h JOIN {}.assets a ON a.id = h.id WHERE h.arm = ?1 GROUP BY a.{field}", entry.l))?;
                let rows = statement.query_map([arm as i64], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
                for row in rows {
                    let (value, count) = row?;
                    *tallies.fields.entry((field.to_string(), value)).or_insert(0) += count;
                }
            }
            if kind.is_some() {
                let mut statement = self.conn.prepare(&format!("SELECT t.tag, COUNT(*) FROM hid h JOIN {}.asset_tags t ON t.asset_id = h.id WHERE h.arm = ?1 GROUP BY t.tag", entry.l))?;
                for row in statement.query_map([arm as i64], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))? {
                    let (value, count) = row?;
                    *tallies.fields.entry(("tag".into(), value)).or_insert(0) += count;
                }
                let mut statement = self.conn.prepare(&format!(
                    "SELECT t.tag, COUNT(*) FROM hid h JOIN {u}.user_tags t ON t.asset_id = h.id \
                     WHERE h.arm = ?1 AND NOT EXISTS (SELECT 1 FROM {l}.asset_tags x WHERE x.asset_id = t.asset_id AND x.tag = t.tag) GROUP BY t.tag",
                    u = entry.u,
                    l = entry.l
                ))?;
                for row in statement.query_map([arm as i64], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))? {
                    let (value, count) = row?;
                    *tallies.fields.entry(("tag".into(), value)).or_insert(0) += count;
                }
            }
        }
        let tallies = Arc::new(tallies);
        let mut cache = TALLIES.lock().unwrap_or_else(|p| p.into_inner());
        if cache.len() >= 16 {
            cache.clear();
        }
        cache.insert(key, tallies.clone());
        Ok(tallies)
    }

    /// The hidden duplicates by field, read from the counts the index keeps (no pass over the libraries). The personal
    /// tags of hidden rows are not in the index (they change without a library revision): counted from `user.db`.
    fn hidden_from_index(&self, kind: Option<&str>) -> AppResult<Tallies> {
        let mut tallies = Tallies::default();
        let mut statement = self.conn.prepare("SELECT type, field, value, n FROM ix.hidfacets WHERE (?1 IS NULL AND field = 'type') OR type = ?1")?;
        for row in statement.query_map([kind], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?)))? {
            let (type_id, field, value, n) = row?;
            let key = if field == "type" { ("type".to_string(), type_id) } else { (field, value) };
            *tallies.fields.entry(key).or_insert(0) += n;
        }
        if let Some(kind) = kind {
            for arm in &self.arms {
                let mut statement = self.conn.prepare(&format!(
                    "SELECT t.tag, COUNT(*) FROM {u}.user_tags t CROSS JOIN ix.shadowed s ON s.lib = ?1 AND s.id = t.asset_id \
                     WHERE s.type = ?2 AND NOT EXISTS (SELECT 1 FROM {l}.asset_tags x WHERE x.asset_id = t.asset_id AND x.tag = t.tag) GROUP BY t.tag",
                    u = arm.u,
                    l = arm.l
                ))?;
                for row in statement.query_map([&arm.id, kind], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))? {
                    let (tag, n) = row?;
                    *tallies.fields.entry(("tag".into(), tag)).or_insert(0) += n;
                }
            }
        }
        Ok(tallies)
    }

    /// Items per type, each once (the sidebar).
    pub fn type_counts(&self, visibility: &Visibility) -> AppResult<Vec<Count>> {
        let hidden = self.hidden(None, visibility)?;
        let mut counts: HashMap<String, i64> = HashMap::new();
        for arm in &self.arms {
            let mut statement = self.conn.prepare(&format!("SELECT type, n FROM {}.facets WHERE field = 'type' AND n > 0", arm.l))?;
            for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))? {
                let (value, count) = row?;
                *counts.entry(value).or_insert(0) += count;
            }
        }
        for ((field, value), count) in &hidden.fields {
            if field == "type" {
                *counts.entry(value.clone()).or_insert(0) -= count;
            }
        }
        let mut out: Vec<Count> = counts.into_iter().filter(|(_, count)| *count > 0).map(|(value, count)| Count { value, count }).collect();
        out.sort_by(|a, b| a.value.cmp(&b.value));
        Ok(out)
    }

    /// Every number the rail shows for `kind`.
    pub fn facet_counts(&self, kind: &str, visibility: &Visibility, tag_limit: usize, selected_tags: &[String]) -> AppResult<FacetCounts> {
        let hidden = self.hidden(Some(kind), visibility)?;
        let mut sums: HashMap<(String, String), i64> = HashMap::new();
        let mut libraries = Vec::new();
        for (index, arm) in self.arms.iter().enumerate() {
            let mut statement = self.conn.prepare(&format!("SELECT field, value, n FROM {}.facets WHERE type = ?1 AND field <> 'tag' AND n > 0", arm.l))?;
            let mut copies = 0;
            for row in statement.query_map([kind], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))? {
                let (field, value, count) = row?;
                if field == "type" {
                    copies = count;
                }
                *sums.entry((field, value)).or_insert(0) += count;
            }
            // What the library holds of the type, every copy, minus what a switched-off plugin hides.
            let mut p = Vec::new();
            if let Some(shown) = Self::visible(visibility, "a", &mut p) {
                p.insert(0, text_value(kind));
                copies = self.conn.query_row(&format!("SELECT COUNT(*) FROM {}.assets a WHERE a.type = ? AND {shown}", arm.l), params_from_iter(p.iter()), |row| row.get(0))?;
            }
            libraries.push(Count { value: arm.id.clone(), count: copies });
            let _ = index;
        }
        for (key, count) in &hidden.fields {
            match key.0.as_str() {
                "tag" => {}
                // The rows of a type are counted under an empty value.
                "type" => *sums.entry(("type".to_string(), String::new())).or_insert(0) -= count,
                _ => *sums.entry(key.clone()).or_insert(0) -= count,
            }
        }
        let group = |field: &str| -> Vec<Count> {
            let mut out: Vec<Count> = sums.iter().filter(|((f, _), count)| f == field && **count > 0).map(|((_, value), count)| Count { value: value.clone(), count: *count }).collect();
            out.sort_by(|a, b| a.value.cmp(&b.value));
            out
        };
        let total = sums.get(&("type".to_string(), String::new())).copied().unwrap_or(0).max(0);
        let (tags, tag_total) = self.tag_counts(kind, &hidden, None, tag_limit, selected_tags)?;
        Ok(FacetCounts {
            total,
            libraries,
            categories: group("category"),
            sources: group("source"),
            publishers: group("publisher"),
            licenses: group("license"),
            formats: group("format"),
            tags,
            tag_total,
        })
    }

    /// Tags of `kind` (each item once, provenance-looking tags left out): the `limit` most used plus the selected ones,
    /// or, with `text`, the `limit` most used among those containing it. Also how many tags there are.
    fn tag_counts(&self, kind: &str, hidden: &Tallies, text: Option<&str>, limit: usize, selected: &[String]) -> AppResult<(Vec<Count>, i64)> {
        let mut sums: HashMap<String, i64> = HashMap::new();
        let needle = text.map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty());
        for arm in &self.arms {
            let mut sql = format!("SELECT value, n FROM {}.facets WHERE type = ? AND field = 'tag' AND n > 0", arm.l);
            let mut p = vec![text_value(kind)];
            for pattern in PROVENANCE_TAG {
                sql.push_str(" AND value NOT LIKE ?");
                p.push(text_value(pattern));
            }
            if let Some(needle) = &needle {
                sql.push_str(" AND value LIKE ? ESCAPE '\\'");
                p.push(text_value(&like_pattern(needle)));
            }
            let mut statement = self.conn.prepare(&sql)?;
            for row in statement.query_map(params_from_iter(p.iter()), |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))? {
                let (value, count) = row?;
                *sums.entry(value).or_insert(0) += count;
            }
            // Personal tags count once for an item that has the same library tag.
            let mut statement = self.conn.prepare(&format!(
                "SELECT t.tag, COUNT(*) FROM {u}.user_tags t CROSS JOIN {l}.assets a ON a.id = t.asset_id WHERE a.type = ?1 \
                 AND NOT EXISTS (SELECT 1 FROM {l}.asset_tags x WHERE x.asset_id = t.asset_id AND x.tag = t.tag) GROUP BY t.tag",
                u = arm.u,
                l = arm.l
            ))?;
            for row in statement.query_map([kind], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))? {
                let (value, count) = row?;
                let lower = value.to_lowercase();
                if PROVENANCE_TAG.iter().any(|pattern| lower.starts_with(&pattern[..pattern.len() - 1])) || needle.as_ref().is_some_and(|n| !lower.contains(n.as_str())) {
                    continue;
                }
                *sums.entry(value).or_insert(0) += count;
            }
        }
        for ((field, value), count) in &hidden.fields {
            if field == "tag" {
                if let Some(sum) = sums.get_mut(value) {
                    *sum -= count;
                }
            }
        }
        sums.retain(|_, count| *count > 0);
        let tag_total = sums.len() as i64;
        let mut all: Vec<Count> = sums.iter().map(|(value, count)| Count { value: value.clone(), count: *count }).collect();
        all.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
        let mut keep: Vec<Count> = all.iter().take(limit).cloned().collect();
        if text.is_none() {
            for tag in selected {
                if !keep.iter().any(|count| &count.value == tag) {
                    keep.push(Count { value: tag.clone(), count: sums.get(tag).copied().unwrap_or(0) });
                }
            }
        }
        keep.sort_by(|a, b| a.value.cmp(&b.value));
        Ok((keep, tag_total))
    }

    /// The tags of `kind` that contain `text`, most used first (at most `limit`), for the rail's search box.
    pub fn search_tags(&self, kind: &str, visibility: &Visibility, text: &str, limit: usize) -> AppResult<Vec<Count>> {
        let hidden = self.hidden(Some(kind), visibility)?;
        Ok(self.tag_counts(kind, &hidden, Some(text), limit, &[])?.0)
    }

    /// Collections (any type) that hold a shown item, and how many items of `kind` each holds.
    pub fn collections(&self, kind: &str, visibility: &Visibility) -> AppResult<Vec<CollectionCount>> {
        let mut counts: HashMap<String, i64> = HashMap::new();
        for (index, arm) in self.arms.iter().enumerate() {
            let mut p: Vec<Value> = vec![text_value(kind)];
            let mut conds: Vec<String> = Vec::new();
            if let Some(shown) = Self::visible(visibility, "a", &mut p) {
                conds.push(shown);
            }
            if let Some(first) = self.first_copy(index, "a", visibility, None, &mut p) {
                conds.push(first);
            }
            let filter = if conds.is_empty() { String::new() } else { format!("WHERE {}", conds.join(" AND ")) };
            let mut statement = self.conn.prepare(&format!(
                "SELECT c.name, SUM(a.type = ?) FROM {u}.user_collections c CROSS JOIN {l}.assets a ON a.id = c.asset_id {filter} GROUP BY c.name",
                u = arm.u,
                l = arm.l
            ))?;
            for row in statement.query_map(params_from_iter(p.iter()), |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))? {
                let (name, count) = row?;
                *counts.entry(name).or_insert(0) += count;
            }
        }
        let mut out: Vec<CollectionCount> = counts.into_iter().map(|(name, count)| CollectionCount { name, count }).collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    // ---- one item -----------------------------------------------------------------

    /// For each `(library, id)`: the copies of the item in other libraries (shown ones), best first,
    /// and the copies that are another version of the same id.
    pub fn elsewhere(&self, items: &[(String, String)], visibility: &Visibility) -> AppResult<Vec<Elsewhere>> {
        let mut out = Vec::new();
        for (library, id) in items {
            let Some(own) = self.arm_index(library) else { continue };
            let found: Option<(String, Option<String>)> = {
                let mut statement = self.conn.prepare_cached(&format!("SELECT type, content_hash FROM {}.assets WHERE id = ?1", self.arms[own].l))?;
                let mut rows = statement.query([id])?;
                match rows.next()? {
                    Some(row) => Some((row.get(0)?, row.get(1)?)),
                    None => None,
                }
            };
            let Some((kind, hash)) = found else { continue };
            let mut entry = Elsewhere { library: library.clone(), id: id.clone(), ..Default::default() };
            for (index, arm) in self.arms.iter().enumerate() {
                if index == own {
                    continue;
                }
                let mut p: Vec<Value> = vec![text_value(&kind)];
                let shown = Self::visible(visibility, "b", &mut p).map(|s| format!(" AND {s}")).unwrap_or_default();
                let mut matches: Vec<String> = Vec::new();
                if let Some(hash) = &hash {
                    let mut q = p.clone();
                    q.insert(1, text_value(hash));
                    let mut statement = self.conn.prepare_cached(&format!("SELECT b.id FROM {}.assets b INDEXED BY assets_hash WHERE b.type = ? AND b.content_hash = ?{shown}", arm.l))?;
                    matches.extend(statement.query_map(params_from_iter(q.iter()), |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?);
                }
                let mut q = p.clone();
                q.insert(1, text_value(id));
                let unhashed = if hash.is_some() { " AND b.content_hash IS NULL" } else { "" };
                let mut statement = self.conn.prepare_cached(&format!("SELECT b.id FROM {}.assets b INDEXED BY sqlite_autoindex_assets_1 WHERE b.type = ? AND b.id = ?{unhashed}{shown}", arm.l))?;
                matches.extend(statement.query_map(params_from_iter(q.iter()), |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?);
                matches.sort();
                matches.dedup();
                entry.also_in.extend(matches.into_iter().map(|id| CopyIn { library: arm.id.clone(), id }));
                if let Some(hash) = &hash {
                    let mut q = p.clone();
                    q.insert(1, text_value(id));
                    q.insert(2, text_value(hash));
                    let mut statement = self.conn.prepare_cached(&format!("SELECT b.id FROM {}.assets b INDEXED BY sqlite_autoindex_assets_1 WHERE b.type = ? AND b.id = ? AND b.content_hash IS NOT NULL AND b.content_hash <> ?{shown}", arm.l))?;
                    for other in statement.query_map(params_from_iter(q.iter()), |row| row.get::<_, String>(0))? {
                        entry.other_versions.push(CopyIn { library: arm.id.clone(), id: other? });
                    }
                }
            }
            out.push(entry);
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum What {
    Collection,
    Favorites,
    Text,
    Tag,
    Field(&'static str),
}

/// A small set of rows a request starts from, in a temporary table of keys (row ids or asset ids).
struct Driver {
    table: String,
    by_rowid: bool,
    what: What,
    /// How many keys it holds.
    size: i64,
}

/// The rows hidden from a count, per `(field, value)` (and `type` when counting every type).
#[derive(Default)]
pub struct Tallies {
    fields: HashMap<(String, String), i64>,
}

static TALLIES: LazyLock<Mutex<HashMap<String, Arc<Tallies>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn fts_phrase(needle: &str) -> String {
    format!("\"{}\"", needle.replace('"', "\"\""))
}
