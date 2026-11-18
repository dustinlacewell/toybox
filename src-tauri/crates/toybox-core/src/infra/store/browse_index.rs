//! The browse index: per-machine derived data that makes the merged, de-duplicated list of several
//! libraries cheap to page through at any depth and cheap to count.
//!
//! It lives in its own file (`<app-data>/browse-index.db`), never in a `library.db`: a local library's
//! database may be shared between computers through a synced folder, and what is a duplicate of what
//! depends on which libraries *this* computer has and how it ranks them. Everything in it is derived
//! from the library databases and can be deleted at any time; it is rebuilt.
//!
//! ```text
//! ix_libs   the libraries (in rank order) and the revision of each that the index has absorbed
//! merged    one row per item of every library: (type, name NOCASE, id, library position, sh)
//!           `sh` is how many better-ranked libraries hold a copy of the item. The unfiltered list of a
//!           type is the rows with sh = 0, in key order: a plain walk of one b-tree.
//! dup       the copy relation, one row per ordered pair (lib, id, by_lib, by_id), both directions
//! shadowed  the rows with sh > 0, with the columns the counts group by (so a removal needs no
//!           lookup of what the row was); triggers keep `hidfacets` (counts of them) right
//! hidfacets (type, field, value) -> how many hidden duplicates: what a count subtracts
//! ```
//!
//! **Copy relation** (`src/domain/duplicates.ts`, as the queries in `browse` apply it): items `x` and
//! `y` of different libraries, same type, are copies when both have a content hash and the hashes are
//! equal, or when they have the same id and either has no hash yet. `x` is hidden when a library that
//! ranks before its own holds a copy.
//!
//! **Maintenance.** [`ensure`] brings the index up to date before a query uses it. For each library
//! whose revision moved, the rows named by its `changes` table since the revision the index last
//! absorbed are removed (merged row, copy pairs, shadow row) and re-added from the library's current
//! rows; the counterparts of the pairs that went or came get their `sh` recomputed. Cost: a few index
//! probes per changed row, however large the libraries. A first build, a changed set or order of
//! libraries (rank change, one added or removed), a replaced database (`dbId`), a revision that went
//! backwards, a feed pruned past what the index absorbed, or counts that disagree with the library's own
//! mean a full rebuild, which is O(rows) and runs on a background thread while queries use the slower
//! per-library path (identical results).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};

use super::browse::{uri_for, Source};
use crate::error::{AppError, AppResult};

pub const INDEX_FILE: &str = "browse-index.db";
const INDEX_VERSION: i64 = 1;
/// More changed rows than this are not absorbed on the caller's thread.
const INLINE_MAX: i64 = 1_000;

const SCHEMA: &str = "
CREATE TABLE ix_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE ix_libs (id TEXT PRIMARY KEY, pos INTEGER NOT NULL, db_id TEXT NOT NULL, revision INTEGER NOT NULL, rows INTEGER NOT NULL) WITHOUT ROWID;
CREATE TABLE merged (
  type TEXT NOT NULL,
  name TEXT NOT NULL COLLATE NOCASE,
  id   TEXT NOT NULL,
  o    INTEGER NOT NULL,
  sh   INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (type, name, id, o)
) WITHOUT ROWID;
CREATE INDEX merged_id ON merged(o, id);
CREATE TABLE dup (
  lib TEXT NOT NULL, id TEXT NOT NULL, by_lib TEXT NOT NULL, by_id TEXT NOT NULL,
  PRIMARY KEY (lib, id, by_lib, by_id)
) WITHOUT ROWID;
CREATE TABLE shadowed (
  lib TEXT NOT NULL, id TEXT NOT NULL, type TEXT NOT NULL,
  category TEXT NOT NULL, source TEXT NOT NULL, publisher TEXT NOT NULL, license TEXT NOT NULL, format TEXT NOT NULL,
  tags TEXT NOT NULL,
  PRIMARY KEY (lib, id)
) WITHOUT ROWID;
CREATE TABLE hidfacets (type TEXT NOT NULL, field TEXT NOT NULL, value TEXT NOT NULL, n INTEGER NOT NULL, PRIMARY KEY (type, field, value)) WITHOUT ROWID;
CREATE TRIGGER shadowed_insert AFTER INSERT ON shadowed BEGIN
  INSERT INTO hidfacets(type,field,value,n) VALUES(NEW.type,'type','',1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
  INSERT INTO hidfacets(type,field,value,n) VALUES(NEW.type,'category',NEW.category,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
  INSERT INTO hidfacets(type,field,value,n) VALUES(NEW.type,'source',NEW.source,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
  INSERT INTO hidfacets(type,field,value,n) VALUES(NEW.type,'publisher',NEW.publisher,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
  INSERT INTO hidfacets(type,field,value,n) VALUES(NEW.type,'license',NEW.license,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
  INSERT INTO hidfacets(type,field,value,n) VALUES(NEW.type,'format',NEW.format,1) ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
  INSERT INTO hidfacets(type,field,value,n) SELECT NEW.type,'tag',j.value,1 FROM json_each(NEW.tags) j WHERE true ON CONFLICT(type,field,value) DO UPDATE SET n=n+1;
END;
CREATE TRIGGER shadowed_delete AFTER DELETE ON shadowed BEGIN
  UPDATE hidfacets SET n=n-1 WHERE type=OLD.type AND ((field='type' AND value='') OR (field='category' AND value=OLD.category) OR (field='source' AND value=OLD.source) OR (field='publisher' AND value=OLD.publisher) OR (field='license' AND value=OLD.license) OR (field='format' AND value=OLD.format));
  UPDATE hidfacets SET n=n-1 WHERE type=OLD.type AND field='tag' AND value IN (SELECT value FROM json_each(OLD.tags));
  DELETE FROM hidfacets WHERE n<=0;
END;
";

/// What [`ensure`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ensured {
    /// The index describes the libraries as they are now.
    Ready,
    /// It is being (re)built on a background thread: use the per-library path meanwhile.
    Pending,
    /// Not worth having (fewer than two libraries).
    Unneeded,
}

/// One library of the index, in rank order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IxLib {
    pub id: String,
    pub pos: i64,
    pub db_id: String,
    pub revision: i64,
}

/// The libraries the index would hold: those with a database, best rank first.
pub fn indexed(all: &[Source]) -> Vec<Source> {
    let mut sources: Vec<Source> = all.iter().filter(|source| source.library.is_file()).cloned().collect();
    sources.sort_by_key(|source| source.rank);
    sources
}

/// Read the libraries recorded in an index database attached as `schema` (or opened as main).
pub(crate) fn read_libs(conn: &Connection, schema: &str) -> AppResult<Vec<IxLib>> {
    let mut statement = conn.prepare(&format!("SELECT id, pos, db_id, revision FROM {schema}.ix_libs ORDER BY pos"))?;
    let rows = statement.query_map([], |row| Ok(IxLib { id: row.get(0)?, pos: row.get(1)?, db_id: row.get(2)?, revision: row.get(3)? }))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

static LOCKS: Mutex<Option<HashMap<PathBuf, Arc<Mutex<()>>>>> = Mutex::new(None);

/// One lock per index file: only one catch-up or build runs at a time.
fn lock_for(index: &Path) -> Arc<Mutex<()>> {
    let mut locks = LOCKS.lock().unwrap_or_else(|p| p.into_inner());
    locks.get_or_insert_with(HashMap::new).entry(index.to_path_buf()).or_default().clone()
}

static BACKGROUND: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// Bring the index at `index` up to date with `all` (the libraries the app has). Small catch-ups run
/// here; a build runs on a background thread (`blocking = false`) and the answer is `Pending`.
/// With `blocking`, everything runs here.
pub fn ensure(index: &Path, all: &[Source], blocking: bool) -> AppResult<Ensured> {
    let sources = indexed(all);
    if sources.len() < 2 {
        return Ok(Ensured::Unneeded);
    }
    let lock = lock_for(index);
    let guard = match lock.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => {
            if !blocking {
                return Ok(Ensured::Pending);
            }
            lock.lock().unwrap_or_else(|p| p.into_inner())
        }
    };
    let mut maintainer = Maintainer::open(index, &sources)?;
    if maintainer.any_loading()? {
        // A replica is still being downloaded: the list is not complete, so there is nothing to describe yet.
        return Ok(Ensured::Pending);
    }
    let plan = maintainer.plan()?;
    match plan {
        Plan::Current => Ok(Ensured::Ready),
        Plan::Incremental(count) if blocking || count <= INLINE_MAX => {
            maintainer.sync()?;
            Ok(Ensured::Ready)
        }
        _ if blocking => {
            maintainer.rebuild()?;
            Ok(Ensured::Ready)
        }
        _ => {
            drop(maintainer);
            drop(guard);
            let mut running = BACKGROUND.lock().unwrap_or_else(|p| p.into_inner());
            if running.get_or_insert_with(HashSet::new).insert(index.to_path_buf()) {
                let (path, owned) = (index.to_path_buf(), sources);
                std::thread::spawn(move || {
                    let _ = ensure(&path, &owned, true);
                    if let Some(set) = BACKGROUND.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                        set.remove(&path);
                    }
                });
            }
            Ok(Ensured::Pending)
        }
    }
}

enum Plan {
    Current,
    /// This many changed rows to absorb (at most; counted up to a bound).
    Incremental(i64),
    Full,
}

/// What a library says about itself now.
struct Now {
    db_id: String,
    revision: i64,
    floor: i64,
    rows: i64,
}

struct Maintainer {
    conn: Connection,
    sources: Vec<Source>,
}

fn schema_error(error: rusqlite::Error) -> AppError {
    AppError::msg(format!("Browse index: {error}"))
}

impl Maintainer {
    fn open(index: &Path, sources: &[Source]) -> AppResult<Maintainer> {
        if let Some(parent) = index.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::file("Create directory", parent, e))?;
        }
        let mut conn = Connection::open_with_flags(index, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(schema_error)?;
        conn.busy_timeout(std::time::Duration::from_secs(15))?;
        conn.execute_batch("PRAGMA synchronous=NORMAL; PRAGMA cache_size=-262144; PRAGMA temp_store=MEMORY;")?;
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != INDEX_VERSION {
            // Nothing in here is the only copy of anything: start again.
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let again: i64 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            if again != INDEX_VERSION {
                let names: Vec<(String, String)> = {
                    let mut statement = tx.prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")?;
                    let found = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
                    found.collect::<Result<_, _>>()?
                };
                for (kind, name) in names.iter().filter(|(kind, _)| kind == "trigger").chain(names.iter().filter(|(kind, _)| kind == "table")) {
                    let _ = tx.execute_batch(&format!("DROP {} IF EXISTS \"{}\"", kind.to_uppercase(), name));
                }
                tx.execute_batch(SCHEMA)?;
                tx.execute_batch(&format!("PRAGMA user_version={INDEX_VERSION}"))?;
            }
            tx.commit()?;
        }
        for (index, source) in sources.iter().enumerate() {
            conn.execute(&format!("ATTACH DATABASE ?1 AS l{index}"), [uri_for(&source.library)]).map_err(|e| AppError::msg(format!("Could not open the database {}: {e}", source.library.display())))?;
        }
        Ok(Maintainer { conn, sources: sources.to_vec() })
    }

    fn any_loading(&self) -> AppResult<bool> {
        for index in 0..self.sources.len() {
            let found: Option<i64> = self.conn.query_row(&format!("SELECT 1 FROM l{index}.meta WHERE key = 'bulkLoad' AND value = '1'"), [], |row| row.get(0)).optional()?;
            if found.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn now(&self, index: usize) -> AppResult<Now> {
        let mut now = Now { db_id: String::new(), revision: 0, floor: 0, rows: 0 };
        let mut statement = self.conn.prepare(&format!("SELECT key, value FROM l{index}.meta WHERE key IN ('dbId','revision','changesFloor')"))?;
        for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))? {
            let (key, value) = row?;
            match key.as_str() {
                "dbId" => now.db_id = value,
                "revision" => now.revision = value.parse().unwrap_or(0),
                _ => now.floor = value.parse().unwrap_or(0),
            }
        }
        now.rows = self.conn.query_row(&format!("SELECT COALESCE(SUM(n), 0) FROM l{index}.facets WHERE field = 'type'"), [], |row| row.get(0))?;
        Ok(now)
    }

    fn plan(&mut self) -> AppResult<Plan> {
        let ready = self.conn.query_row("SELECT value FROM ix_meta WHERE key = 'ready'", [], |row| row.get::<_, String>(0)).optional()?.is_some();
        let stored = read_libs(&self.conn, "main")?;
        let rows: HashMap<String, i64> = {
            let mut statement = self.conn.prepare("SELECT id, rows FROM ix_libs")?;
            let found = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
            found.collect::<Result<_, _>>()?
        };
        if !ready || stored.len() != self.sources.len() || stored.iter().zip(&self.sources).any(|(have, source)| have.id != source.id) {
            return Ok(Plan::Full);
        }
        let mut changed = 0;
        for (index, have) in stored.iter().enumerate() {
            let now = self.now(index)?;
            if now.db_id != have.db_id || now.revision < have.revision || (now.revision > have.revision && now.floor > have.revision) {
                return Ok(Plan::Full);
            }
            if now.revision == have.revision {
                if now.rows != rows.get(&have.id).copied().unwrap_or(-1) {
                    return Ok(Plan::Full);
                }
                continue;
            }
            let bounded: i64 = self.conn.query_row(
                &format!("SELECT COUNT(*) FROM (SELECT 1 FROM l{index}.changes WHERE revision > ?1 AND revision <= ?2 LIMIT {})", INLINE_MAX + 1),
                params![have.revision, now.revision],
                |row| row.get(0),
            )?;
            changed += bounded;
        }
        Ok(if changed == 0 { Plan::Current } else { Plan::Incremental(changed) })
    }

    fn positions(&self) -> HashMap<String, i64> {
        self.sources.iter().enumerate().map(|(index, source)| (source.id.clone(), index as i64)).collect()
    }

    // ---- a full build ---------------------------------------------------------------

    fn rebuild(&mut self) -> AppResult<()> {
        #[cfg(test)]
        REBUILDS.with(|count| count.set(count.get() + 1));
        let sources = self.sources.clone();
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let snapshot: Vec<Now> = (0..sources.len())
            .map(|index| {
                let mut now = Now { db_id: String::new(), revision: 0, floor: 0, rows: 0 };
                let mut statement = tx.prepare(&format!("SELECT key, value FROM l{index}.meta WHERE key IN ('dbId','revision','changesFloor')"))?;
                for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))? {
                    let (key, value) = row?;
                    match key.as_str() {
                        "dbId" => now.db_id = value,
                        "revision" => now.revision = value.parse().unwrap_or(0),
                        _ => now.floor = value.parse().unwrap_or(0),
                    }
                }
                Ok(now)
            })
            .collect::<AppResult<_>>()?;
        tx.execute_batch("DELETE FROM ix_meta; DELETE FROM ix_libs; DELETE FROM shadowed; DELETE FROM hidfacets; DELETE FROM dup; DELETE FROM merged; DROP INDEX IF EXISTS merged_id;")?;
        // The libraries' rows, interleaved in key order so the b-tree is filled left to right.
        let arms: Vec<String> = (0..sources.len()).map(|index| format!("SELECT type, name, id, {index} AS o, 0 AS sh FROM l{index}.assets")).collect();
        tx.execute_batch(&format!("INSERT INTO merged(type, name, id, o, sh) SELECT type, name, id, o, sh FROM ({}) ORDER BY type, name COLLATE NOCASE, id, o", arms.join(" UNION ALL ")))?;
        tx.execute_batch("CREATE INDEX merged_id ON merged(o, id)")?;
        for (index, source) in sources.iter().enumerate() {
            let rows: i64 = tx.query_row(&format!("SELECT COALESCE(SUM(n), 0) FROM l{index}.facets WHERE field = 'type'"), [], |row| row.get(0))?;
            tx.execute("INSERT INTO ix_libs(id, pos, db_id, revision, rows) VALUES(?1, ?2, ?3, ?4, ?5)", params![source.id, index as i64, snapshot[index].db_id, snapshot[index].revision, rows])?;
        }
        // Copies: for each pair of libraries, by hash and by id.
        tx.execute_batch("CREATE TEMP TABLE IF NOT EXISTS pr (a TEXT NOT NULL, b TEXT NOT NULL, PRIMARY KEY (a, b)) WITHOUT ROWID; DELETE FROM pr;")?;
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                tx.execute_batch(&format!(
                    "INSERT OR IGNORE INTO pr SELECT a.id, b.id FROM l{i}.assets a CROSS JOIN l{j}.assets b INDEXED BY assets_hash ON b.type = a.type AND b.content_hash = a.content_hash WHERE a.content_hash IS NOT NULL;
                     INSERT OR IGNORE INTO pr SELECT a.id, b.id FROM l{i}.assets a INDEXED BY assets_unhashed CROSS JOIN l{j}.assets b ON b.id = a.id AND b.type = a.type WHERE a.content_hash IS NULL;
                     INSERT OR IGNORE INTO pr SELECT a.id, b.id FROM l{j}.assets b INDEXED BY assets_unhashed CROSS JOIN l{i}.assets a ON a.id = b.id AND a.type = b.type WHERE b.content_hash IS NULL;"
                ))?;
                tx.execute("INSERT OR IGNORE INTO dup(lib, id, by_lib, by_id) SELECT ?1, a, ?2, b FROM pr", params![sources[i].id, sources[j].id])?;
                tx.execute("INSERT OR IGNORE INTO dup(lib, id, by_lib, by_id) SELECT ?2, b, ?1, a FROM pr", params![sources[i].id, sources[j].id])?;
                tx.execute_batch("DELETE FROM pr")?;
            }
        }
        // sh: how many copies sit in a library that ranks before the row's own.
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS shn (pos INTEGER NOT NULL, id TEXT NOT NULL, n INTEGER NOT NULL, PRIMARY KEY (pos, id)) WITHOUT ROWID; DELETE FROM shn;
             INSERT INTO shn SELECT ly.pos, d.id, COUNT(*) FROM dup d JOIN ix_libs lb ON lb.id = d.by_lib JOIN ix_libs ly ON ly.id = d.lib WHERE lb.pos < ly.pos GROUP BY ly.pos, d.id;
             UPDATE merged SET sh = (SELECT n FROM shn WHERE shn.pos = merged.o AND shn.id = merged.id) WHERE EXISTS (SELECT 1 FROM shn WHERE shn.pos = merged.o AND shn.id = merged.id);",
        )?;
        for (index, source) in sources.iter().enumerate() {
            tx.execute(
                &format!(
                    "INSERT OR IGNORE INTO shadowed(lib, id, type, category, source, publisher, license, format, tags) \
                     SELECT ?1, a.id, a.type, a.category, a.source, a.publisher, a.license, a.format, (SELECT json_group_array(t.tag) FROM l{index}.asset_tags t WHERE t.asset_id = a.id) \
                     FROM shn s CROSS JOIN l{index}.assets a ON a.id = s.id WHERE s.pos = ?2"
                ),
                params![source.id, index as i64],
            )?;
        }
        tx.execute_batch("DROP TABLE IF EXISTS temp.pr; DROP TABLE IF EXISTS temp.shn;")?;
        tx.execute("INSERT INTO ix_meta(key, value) VALUES('ready', '1')", [])?;
        tx.commit()?;
        Ok(())
    }

    // ---- catching up ----------------------------------------------------------------

    fn sync(&mut self) -> AppResult<()> {
        let sources = self.sources.clone();
        let positions = self.positions();
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored = read_libs(&tx, "main")?;
        let mut now = Vec::new();
        for index in 0..sources.len() {
            let mut one = Now { db_id: String::new(), revision: 0, floor: 0, rows: 0 };
            let mut statement = tx.prepare(&format!("SELECT key, value FROM l{index}.meta WHERE key IN ('dbId','revision','changesFloor')"))?;
            for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))? {
                let (key, value) = row?;
                match key.as_str() {
                    "dbId" => one.db_id = value,
                    "revision" => one.revision = value.parse().unwrap_or(0),
                    _ => one.floor = value.parse().unwrap_or(0),
                }
            }
            one.rows = tx.query_row(&format!("SELECT COALESCE(SUM(n), 0) FROM l{index}.facets WHERE field = 'type'"), [], |row| row.get(0))?;
            now.push(one);
        }
        // Who changed, per library.
        let mut changed: Vec<Vec<String>> = Vec::new();
        for (index, have) in stored.iter().enumerate() {
            let mut ids = Vec::new();
            if now[index].revision > have.revision {
                let mut statement = tx.prepare(&format!("SELECT asset_id FROM l{index}.changes WHERE revision > ?1 AND revision <= ?2"))?;
                for row in statement.query_map(params![have.revision, now[index].revision], |row| row.get::<_, String>(0))? {
                    ids.push(row?);
                }
            }
            changed.push(ids);
        }
        let mut delta_rows: Vec<i64> = vec![0; sources.len()];
        let mut affected: HashSet<(usize, String)> = HashSet::new();
        // First everything that leaves ...
        for (index, ids) in changed.iter().enumerate() {
            let lib = &sources[index].id;
            for id in ids {
                let old: Option<(String, String)> = tx.prepare_cached("SELECT type, name FROM merged WHERE o = ?1 AND id = ?2")?.query_row(params![index as i64, id], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
                if let Some((kind, name)) = old {
                    tx.prepare_cached("DELETE FROM merged WHERE type = ?1 AND name = ?2 AND id = ?3 AND o = ?4")?.execute(params![kind, name, id, index as i64])?;
                    delta_rows[index] -= 1;
                }
                let others: Vec<(String, String)> = {
                    let mut statement = tx.prepare_cached("SELECT by_lib, by_id FROM dup WHERE lib = ?1 AND id = ?2")?;
                    let found = statement.query_map(params![lib, id], |row| Ok((row.get(0)?, row.get(1)?)))?;
                    found.collect::<Result<_, _>>()?
                };
                tx.prepare_cached("DELETE FROM dup WHERE lib = ?1 AND id = ?2")?.execute(params![lib, id])?;
                for (other_lib, other_id) in others {
                    tx.prepare_cached("DELETE FROM dup WHERE lib = ?1 AND id = ?2 AND by_lib = ?3 AND by_id = ?4")?.execute(params![other_lib, other_id, lib, id])?;
                    if let Some(position) = positions.get(&other_lib) {
                        affected.insert((*position as usize, other_id));
                    }
                }
                tx.prepare_cached("DELETE FROM shadowed WHERE lib = ?1 AND id = ?2")?.execute(params![lib, id])?;
            }
        }
        // ... then everything that arrives, against the other libraries as they are now.
        for (index, ids) in changed.iter().enumerate() {
            let lib = &sources[index].id;
            for id in ids {
                let found: Option<(String, String, Option<String>)> =
                    tx.prepare_cached(&format!("SELECT type, name, content_hash FROM l{index}.assets WHERE id = ?1"))?.query_row([id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
                let Some((kind, name, hash)) = found else { continue };
                tx.prepare_cached("INSERT INTO merged(type, name, id, o, sh) VALUES(?1, ?2, ?3, ?4, 0)")?.execute(params![kind, name, id, index as i64])?;
                delta_rows[index] += 1;
                affected.insert((index, id.clone()));
                for other in 0..sources.len() {
                    if other == index {
                        continue;
                    }
                    let mut copies: Vec<String> = Vec::new();
                    if let Some(hash) = &hash {
                        let mut statement = tx.prepare_cached(&format!("SELECT id FROM l{other}.assets INDEXED BY assets_hash WHERE type = ?1 AND content_hash = ?2"))?;
                        for row in statement.query_map(params![kind, hash], |row| row.get::<_, String>(0))? {
                            copies.push(row?);
                        }
                    }
                    let same_id: Option<Option<String>> = tx
                        .prepare_cached(&format!("SELECT content_hash FROM l{other}.assets INDEXED BY sqlite_autoindex_assets_1 WHERE id = ?1 AND type = ?2"))?
                        .query_row(params![id, kind], |row| row.get::<_, Option<String>>(0))
                        .optional()?;
                    if let Some(their_hash) = same_id {
                        if hash.is_none() || their_hash.is_none() {
                            copies.push(id.clone());
                        }
                    }
                    copies.sort();
                    copies.dedup();
                    for copy in copies {
                        tx.prepare_cached("INSERT OR IGNORE INTO dup(lib, id, by_lib, by_id) VALUES(?1, ?2, ?3, ?4)")?.execute(params![lib, id, sources[other].id, copy])?;
                        tx.prepare_cached("INSERT OR IGNORE INTO dup(lib, id, by_lib, by_id) VALUES(?1, ?2, ?3, ?4)")?.execute(params![sources[other].id, copy, lib, id])?;
                        affected.insert((other, copy));
                    }
                }
            }
        }
        // Recompute sh of every row a pair came or went for, and the shadow rows.
        for (index, id) in &affected {
            let mine = *index as i64;
            let better: i64 = {
                let mut statement = tx.prepare_cached("SELECT by_lib FROM dup WHERE lib = ?1 AND id = ?2")?;
                let libs = statement.query_map(params![sources[*index].id, id], |row| row.get::<_, String>(0))?;
                let mut n = 0;
                for lib in libs {
                    if positions.get(&lib?).is_some_and(|position| *position < mine) {
                        n += 1;
                    }
                }
                n
            };
            tx.prepare_cached("UPDATE merged SET sh = ?3 WHERE o = ?1 AND id = ?2 AND sh <> ?3")?.execute(params![mine, id, better])?;
            if better == 0 {
                tx.prepare_cached("DELETE FROM shadowed WHERE lib = ?1 AND id = ?2")?.execute(params![sources[*index].id, id])?;
            } else {
                tx.prepare_cached(&format!(
                    "INSERT OR IGNORE INTO shadowed(lib, id, type, category, source, publisher, license, format, tags) \
                     SELECT ?1, a.id, a.type, a.category, a.source, a.publisher, a.license, a.format, (SELECT json_group_array(t.tag) FROM l{index}.asset_tags t WHERE t.asset_id = a.id) FROM l{index}.assets a WHERE a.id = ?2"
                ))?
                .execute(params![sources[*index].id, id])?;
            }
        }
        // Remember where each library stands; a count that does not add up means the feed missed something.
        let mut sound = true;
        for (index, have) in stored.iter().enumerate() {
            let rows = tx.query_row("SELECT rows FROM ix_libs WHERE id = ?1", [&have.id], |row| row.get::<_, i64>(0))? + delta_rows[index];
            if rows != now[index].rows {
                sound = false;
            }
            tx.execute("UPDATE ix_libs SET revision = ?2, db_id = ?3, rows = ?4 WHERE id = ?1", params![have.id, now[index].revision, now[index].db_id, now[index].rows])?;
        }
        if !sound {
            tx.rollback()?;
            return self.rebuild();
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    /// Full builds run on this thread (tests assert that a catch-up was not one).
    pub(crate) static REBUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
