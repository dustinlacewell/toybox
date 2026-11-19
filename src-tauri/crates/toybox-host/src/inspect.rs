//! Looking at a library: its numbers, a consistency check and read-only SQL.

use std::collections::HashSet;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use toybox_core::domain::paths::abs_under_root;
use toybox_core::domain::sources;
use toybox_core::error::{AppError, AppResult};
use toybox_core::infra::store::browse::{Count, Visibility};
use toybox_core::infra::store::Store;
use toybox_core::ops::browse::{self, BrowseLibrary};

use crate::host::Host;

const PAGE: usize = 500;

/// The numbers `tbx library info` shows.
#[derive(Debug, Clone)]
pub struct LibraryStats {
    pub items: i64,
    pub revision: i64,
    /// Items per type.
    pub types: Vec<Count>,
    /// Files the items are made of (each counted once) and their total size; `None` for
    /// a remote library, whose files are fetched when needed.
    pub files: Option<u64>,
    pub bytes: Option<u64>,
    /// Bytes of downloaded files of a remote library.
    pub cache_bytes: Option<u64>,
    /// Kept source files, each stored once (shared storage): how many, their bytes, and the bytes sharing
    /// saves against a copy for every item that keeps one. `None` for a remote library.
    pub source_files: Option<u64>,
    pub source_bytes: Option<u64>,
    pub source_saved_bytes: Option<u64>,
    /// What the migration to shared sources did (items converted, bytes freed), if it ran here.
    pub migration: Option<toybox_core::infra::source_migration::SourceMigration>,
}

/// One thing `check` found wrong.
#[derive(Debug, Clone)]
pub struct CheckProblem {
    /// The item it concerns, when it concerns one.
    pub item: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    pub items: i64,
    pub files_checked: u64,
    pub problems: Vec<CheckProblem>,
    /// Shared source files no item uses (not a problem: `fix` removes them).
    pub orphan_sources: Vec<String>,
    /// Orphans `fix` removed.
    pub removed_sources: Vec<String>,
}

/// A table of query results.
#[derive(Debug, Clone)]
pub struct SqlTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    /// More rows matched than `limit` allowed.
    pub truncated: bool,
}

impl Host {
    /// How many items a library has (it must have been opened).
    pub fn item_count(&self, library: &str) -> AppResult<i64> {
        Store::for_handle(&self.resolve(library)?).asset_count()
    }

    /// The numbers of a library (it is opened first, as for reading).
    pub fn library_stats(&self, library: &str) -> AppResult<LibraryStats> {
        self.open_library(library, false)?;
        let handle = self.resolve(library)?;
        let store = Store::for_handle(&handle);
        let types = browse::type_counts(&[BrowseLibrary { handle: handle.clone(), rank: 0 }], &Visibility::default())?;
        let (mut files, mut bytes) = (None, None);
        let (mut source_files, mut source_bytes, mut source_saved) = (None, None, None);
        if !handle.is_remote() {
            let (mut seen, mut size) = (HashSet::new(), 0u64);
            // Each shared source: how many items keep it, and how big it is.
            let mut shared: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();
            for_each_page(&store, |assets| {
                for asset in assets {
                    for item in std::iter::once(asset).chain(&asset.variants) {
                        for path in item.origin.iter().flat_map(|origin| origin.files.iter()).filter(|path| sources::is_shared(path)) {
                            let length = std::fs::metadata(abs_under_root(&handle.root_str(), path)).map(|meta| meta.len()).unwrap_or(0);
                            shared.entry(path.clone()).or_insert((0, length)).0 += 1;
                        }
                    }
                    let mut paths = Vec::new();
                    asset.collect_files(&mut paths);
                    for path in paths {
                        if seen.insert(path.clone()) {
                            size += std::fs::metadata(abs_under_root(&handle.root_str(), &path)).map(|meta| meta.len()).unwrap_or(0);
                        }
                    }
                }
            })?;
            files = Some(seen.len() as u64);
            bytes = Some(size);
            source_files = Some(shared.len() as u64);
            source_bytes = Some(shared.values().map(|(_, length)| length).sum());
            source_saved = Some(shared.values().map(|(count, length)| count.saturating_sub(1) * length).sum());
        }
        let cache_bytes = if handle.is_remote() { Some(self.remote_cache_size(library)?) } else { None };
        let summary = browse::summary(&handle)?;
        let migration = toybox_core::infra::source_migration::report(&store).filter(|report| report.items > 0 || !report.skipped.is_empty());
        Ok(LibraryStats { items: summary.count, revision: summary.revision, types, files, bytes, cache_bytes, source_files, source_bytes, source_saved_bytes: source_saved, migration })
    }

    /// Verify the library database and (for a local library) that every file of every
    /// item is there.
    pub fn check_library(&self, library: &str, fix: bool) -> AppResult<CheckReport> {
        let handle = self.resolve(library)?;
        let mut report = CheckReport::default();
        let database = handle.library_db_path();
        if !database.is_file() {
            // A library that was never opened has no database yet; opening builds it.
            self.open_library(library, false)?;
        }
        if let Ok(connection) = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            let mut statement = connection.prepare("PRAGMA integrity_check").map_err(AppError::from)?;
            let lines: Vec<String> = statement.query_map([], |row| row.get::<_, String>(0)).map_err(AppError::from)?.filter_map(Result::ok).collect();
            if lines != ["ok"] {
                for line in lines {
                    report.problems.push(CheckProblem { item: None, message: format!("database: {line}") });
                }
            }
        } else {
            report.problems.push(CheckProblem { item: None, message: format!("the library database {} cannot be opened", database.display()) });
            return Ok(report);
        }
        self.open_library(library, false)?;
        let store = Store::for_handle(&handle);
        report.items = store.asset_count()?;
        if handle.is_remote() {
            return Ok(report);
        }
        let root = handle.root_str();
        let mut checked = HashSet::new();
        let mut referenced: HashSet<String> = HashSet::new();
        for_each_page(&store, |assets| {
            for asset in assets {
                let mut paths = Vec::new();
                asset.collect_files(&mut paths);
                for path in paths {
                    if sources::is_shared(&path) {
                        referenced.insert(path.to_lowercase());
                    }
                    if !checked.insert(path.clone()) {
                        continue;
                    }
                    let file = abs_under_root(&root, &path);
                    if !file.is_file() {
                        let what = if sources::is_shared(&path) { "missing source file" } else { "missing file" };
                        report.problems.push(CheckProblem { item: Some(asset.id.clone()), message: format!("{} ({}): {what} {path}", asset.name, asset.id) });
                    } else if let Some((_, hash, _)) = sources::parse_shared(&path) {
                        // A shared source is named by its content: it can be verified.
                        if hash_of(&file).is_some_and(|actual| actual != hash) {
                            report.problems.push(CheckProblem { item: Some(asset.id.clone()), message: format!("{} ({}): source file {path} does not match its name (it is damaged)", asset.name, asset.id) });
                        }
                    }
                }
            }
        })?;
        report.files_checked = checked.len() as u64;
        for orphan in shared_sources_on_disk(&handle.root).into_iter().filter(|path| !referenced.contains(&path.to_lowercase())) {
            if fix && !store.files_in_use(std::slice::from_ref(&orphan))?.contains(&orphan) {
                let file = abs_under_root(&root, &orphan);
                if std::fs::remove_file(&file).is_ok() {
                    toybox_core::infra::retention::remove_set_folder_if_empty(&file);
                    report.removed_sources.push(orphan);
                    continue;
                }
            }
            report.orphan_sources.push(orphan);
        }
        Ok(report)
    }

    /// Run one read-only SQL query on a library's database (the schema is not a stable interface).
    /// `limit` is the most rows returned (0: all).
    pub fn query_sql(&self, library: &str, sql: &str, limit: usize) -> AppResult<SqlTable> {
        self.open_library(library, false)?;
        let handle = self.resolve(library)?;
        let database = handle.library_db_path();
        if !single_statement(sql) {
            return Err(AppError::msg("give one SQL statement"));
        }
        let connection = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        connection.execute_batch("PRAGMA query_only = ON")?;
        let mut statement = connection.prepare(sql)?;
        if !statement.readonly() {
            return Err(AppError::msg("only queries that read are allowed"));
        }
        let columns: Vec<String> = statement.column_names().into_iter().map(String::from).collect();
        let mut rows = statement.query([])?;
        let (mut out, mut truncated) = (Vec::new(), false);
        while let Some(row) = rows.next()? {
            if limit > 0 && out.len() >= limit {
                truncated = true;
                break;
            }
            let mut values = Vec::with_capacity(columns.len());
            for index in 0..columns.len() {
                values.push(match row.get_ref(index)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(number) => Value::from(number),
                    ValueRef::Real(number) => serde_json::Number::from_f64(number).map_or(Value::Null, Value::Number),
                    ValueRef::Text(text) => Value::String(String::from_utf8_lossy(text).into_owned()),
                    ValueRef::Blob(bytes) => Value::String(bytes.iter().map(|byte| format!("{byte:02x}")).collect()),
                });
            }
            out.push(values);
        }
        Ok(SqlTable { columns, rows: out, truncated })
    }
}

/// The SHA-256 of a file, or `None` when it cannot be read.
fn hash_of(path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let count = file.read(&mut buffer).ok()?;
        if count == 0 {
            return Some(format!("{:x}", hash.finalize()));
        }
        hash.update(&buffer[..count]);
    }
}

/// Library-relative paths of the shared source files that are on disk.
fn shared_sources_on_disk(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    for folder in ["materials", "models"] {
        let base = root.join("library").join(folder).join(sources::SOURCES_DIR);
        let Ok(hashes) = std::fs::read_dir(&base) else { continue };
        for hash in hashes.flatten() {
            let Ok(files) = std::fs::read_dir(hash.path()) else { continue };
            for file in files.flatten() {
                if file.file_type().is_ok_and(|kind| kind.is_file()) {
                    out.push(format!("library/{folder}/{}/{}/{}", sources::SOURCES_DIR, hash.file_name().to_string_lossy(), file.file_name().to_string_lossy()));
                }
            }
        }
    }
    out.sort();
    out
}

/// Visit every top-level catalog asset, a page at a time.
fn for_each_page(store: &Store, mut visit: impl FnMut(&[toybox_core::domain::catalog_model::Asset])) -> AppResult<()> {
    let mut after = String::new();
    loop {
        let (ids, next) = store.asset_id_page(&after, PAGE)?;
        if ids.is_empty() {
            return Ok(());
        }
        visit(&store.load_top_assets(&ids)?);
        match next {
            Some(next) => after = next,
            None => return Ok(()),
        }
    }
}

/// Whether `sql` is one statement (a trailing `;` is fine; a `;` inside quotes is not a separator).
fn single_statement(sql: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut chars = sql.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"' | '`') => quote = Some(c),
            (None, ';') => return chars.all(char::is_whitespace),
            _ => {}
        }
    }
    !sql.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::single_statement;

    #[test]
    fn one_statement_only() {
        assert!(single_statement("select 1"));
        assert!(single_statement("select 1;  "));
        assert!(single_statement("select ';'"));
        assert!(!single_statement("select 1; select 2"));
        assert!(!single_statement("  "));
    }
}
