//! Consistent snapshots of the library database, taken before a migration that rewrites entries.
//!
//! A snapshot is `library.db` alone (SQLite's `VACUUM INTO`, a consistent copy even while the database
//! is in use) in `_library_config/snapshots/library-v<from-version>-<UTC time>.db`; the newest
//! [`KEEP`] are kept. It holds the library's entries and everything derived from them as they were.
//!
//! What it can and cannot restore: **entries yes, files no.** A migration that moves files (step 5
//! moves kept source files into shared storage and deletes the copies beside the items) leaves a
//! snapshot whose entries name files that are no longer there. [`restore`] therefore checks, before it
//! changes anything, that every file the snapshot's entries reference exists, and refuses when one is
//! missing. Personal data (`user.db`, favourites, collections) is not part of it.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::error::{AppError, AppResult};

/// Snapshots live here, below the library folder.
pub const DIR: &str = "_library_config/snapshots";
/// How many snapshots are kept.
pub const KEEP: usize = 3;

/// One snapshot on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub name: String,
    pub path: PathBuf,
    pub bytes: u64,
    /// The migration version the library was at when it was taken.
    pub from_version: u32,
    /// `YYYYMMDD-HHMMSS`, UTC.
    pub taken: String,
}

/// `YYYYMMDD-HHMMSS` (UTC) for a Unix time.
pub fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}{month:02}{day:02}-{:02}{:02}{:02}", rest / 3600, rest % 3600 / 60, rest % 60)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn database(root: &Path) -> PathBuf {
    root.join(crate::config::LIBRARY_DB_REL)
}

/// Where a snapshot taken now at `from_version` would go (it is not created).
pub fn planned_path(root: &Path, from_version: u32) -> PathBuf {
    root.join(DIR).join(format!("library-v{from_version}-{}.db", utc_stamp(now())))
}

/// Take a snapshot of `library.db` and drop the oldest beyond [`KEEP`]. Returns its path.
pub fn take(root: &Path, from_version: u32) -> AppResult<PathBuf> {
    let source = database(root);
    if !source.is_file() {
        return Err(AppError::msg("There is no library database to take a snapshot of"));
    }
    let folder = root.join(DIR);
    std::fs::create_dir_all(&folder).map_err(|e| AppError::file("Create directory", &folder, e))?;
    // Names are by the second; two in one second take the next free second, so the order stays the time order.
    let mut at = now();
    let existing = list(root);
    if let Some(last) = existing.first() {
        while utc_stamp(at) <= last.taken {
            at += 1;
        }
    }
    let target = folder.join(format!("library-v{from_version}-{}.db", utc_stamp(at)));
    let pending = target.with_extension("db.partial");
    let _ = std::fs::remove_file(&pending);
    let connection = Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    connection.busy_timeout(std::time::Duration::from_secs(30))?;
    let text = pending.to_string_lossy().to_string();
    let made = connection.execute("VACUUM INTO ?1", [&text]);
    drop(connection);
    if let Err(error) = made {
        let _ = std::fs::remove_file(&pending);
        return Err(error.into());
    }
    // The snapshot is for the Toybox that wrote the library at `from_version`: the database schema
    // that version reads. Opening the library with this build upgrades `library.db` in place (schema 2
    // adds a derived column) before any format step runs, so the copy is brought back down.
    if let Err(error) = downgrade_for(&pending, from_version) {
        let _ = std::fs::remove_file(&pending);
        return Err(error);
    }
    std::fs::rename(&pending, &target).map_err(|e| AppError::file("Name the snapshot", &target, e))?;
    prune(root);
    Ok(target)
}

/// The `library.db` schema the Toybox releases of a library format version read: formats up to 5 (Toybox
/// 0.5.x) read schema 1; format 6 added schema 2 (`assets.derived_from`).
pub fn schema_for_format(format_version: u32) -> i64 {
    if format_version <= 5 { 1 } else { crate::infra::store::SCHEMA_VERSION }
}

/// Bring a snapshot copy down to the schema of `from_version` (see [`schema_for_format`]). Schema 2 only
/// added `assets.derived_from` and its index, both derived from the stored asset JSON, so dropping them
/// loses nothing.
fn downgrade_for(path: &Path, from_version: u32) -> AppResult<()> {
    let wanted = schema_for_format(from_version);
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version <= wanted {
        return Ok(());
    }
    if wanted < 2 {
        connection.execute_batch("DROP INDEX IF EXISTS assets_derived_from;")?;
        let has: i64 = connection.query_row("SELECT COUNT(*) FROM pragma_table_info('assets') WHERE name='derived_from'", [], |row| row.get(0))?;
        if has > 0 {
            connection.execute_batch("ALTER TABLE assets DROP COLUMN derived_from;")?;
        }
    }
    connection.execute_batch(&format!("PRAGMA user_version={wanted};"))?;
    Ok(())
}

/// The snapshots, newest first.
pub fn list(root: &Path) -> Vec<Snapshot> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(root.join(DIR)) else { return out };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("library-v").and_then(|rest| rest.strip_suffix(".db")) else { continue };
        let Some((version, taken)) = rest.split_once('-') else { continue };
        let Ok(from_version) = version.parse::<u32>() else { continue };
        let bytes = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        let taken = taken.to_string();
        out.push(Snapshot { name, path: entry.path(), bytes, from_version, taken });
    }
    out.sort_by(|a, b| b.taken.cmp(&a.taken).then_with(|| b.name.cmp(&a.name)));
    out
}

fn prune(root: &Path) {
    for old in list(root).into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(old.path);
    }
}

/// A read-only connection that changes nothing on disk: the file is opened `immutable` unless a write-ahead log
/// holds changes that are not in it yet.
pub fn open_unchanged(path: &Path) -> AppResult<Connection> {
    let wal = path.with_file_name(format!("{}-wal", path.file_name().and_then(|n| n.to_str()).unwrap_or("library.db")));
    if wal.metadata().map(|meta| meta.len() > 0).unwrap_or(false) {
        return Ok(Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?);
    }
    let mut uri = String::from("file:");
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_string();
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'%' => uri.push_str("%25"),
            b'?' => uri.push_str("%3f"),
            b'#' => uri.push_str("%23"),
            b' ' => uri.push_str("%20"),
            other => uri.push(other as char),
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    Ok(Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX)?)
}

/// What [`restore`] did.
#[derive(Debug, Clone)]
pub struct Restored {
    pub snapshot: Snapshot,
    /// The database that was replaced, kept beside the new one.
    pub previous: PathBuf,
}

/// Put a snapshot's entries back as the library database. The caller makes sure no server or app is using
/// the library. Refuses when a file the snapshot's entries reference is missing (a migration that moved
/// files cannot be undone from a snapshot). The database that was in use is kept as
/// `library.db.before-restore-<time>`; the snapshot itself is kept.
pub fn restore(root: &Path, name: &str) -> AppResult<Restored> {
    let snapshot = list(root).into_iter().find(|snapshot| snapshot.name == name).ok_or_else(|| AppError::msg(format!("There is no snapshot named {name}")))?;
    let connection = open_unchanged(&snapshot.path)?;
    let mut statement = connection.prepare("SELECT DISTINCT path FROM asset_files")?;
    let mut paths: Vec<String> = statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<_, _>>()?;
    drop(statement);
    let mut statement = connection.prepare("SELECT entry FROM items WHERE instr(entry, '\"origin\"') > 0")?;
    let entries: Vec<String> = statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<_, _>>()?;
    drop(statement);
    drop(connection);
    for text in entries {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        for item in std::iter::once(&entry).chain(entry["variants"].as_array().into_iter().flatten()) {
            let folder = item["storagePath"].as_str().or_else(|| entry["storagePath"].as_str()).unwrap_or("");
            paths.extend(item["origin"]["files"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|file| {
                if file.contains('/') || folder.is_empty() { file.to_string() } else { format!("{folder}/{file}") }
            }));
        }
    }
    paths.sort();
    paths.dedup();
    let missing: Vec<&String> = paths.iter().filter(|path| !crate::domain::paths::abs_under_root(&root.to_string_lossy(), path).is_file()).collect();
    if !missing.is_empty() {
        let shown: Vec<&str> = missing.iter().take(5).map(|path| path.as_str()).collect();
        return Err(AppError::msg(format!(
            "Not restored: {} file(s) the snapshot's entries name are no longer there (for example {}). A migration moved or removed them; a snapshot restores entries, not files.",
            missing.len(),
            shown.join(", ")
        )));
    }
    let live = database(root);
    let previous = root.join(format!("{}.before-restore-{}", crate::config::LIBRARY_DB_REL, utc_stamp(now())));
    if live.is_file() {
        let connection = Connection::open_with_flags(&live, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        connection.execute("VACUUM INTO ?1", [&previous.to_string_lossy().to_string()])?;
    }
    let staged = live.with_extension("db.restoring");
    std::fs::copy(&snapshot.path, &staged).map_err(|e| AppError::file("Copy the snapshot", &staged, e))?;
    for suffix in ["-wal", "-shm"] {
        let side = live.with_file_name(format!("{}{suffix}", live.file_name().and_then(|n| n.to_str()).unwrap_or("library.db")));
        let _ = std::fs::remove_file(side);
    }
    std::fs::rename(&staged, &live).map_err(|e| AppError::file("Replace the library database", &live, e))?;
    // The library is at the snapshot's version again: an older Toybox may open it.
    let journal = root.join("_library_config/migrations.json");
    if let Ok(bytes) = std::fs::read(&journal) {
        if let Ok(mut doc) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            if doc["version"].as_u64().is_some_and(|v| v > snapshot.from_version as u64) {
                doc["version"] = serde_json::json!(snapshot.from_version);
                if let Some(applied) = doc["applied"].as_array_mut() {
                    applied.retain(|step| step["version"].as_u64().is_some_and(|v| v <= snapshot.from_version as u64));
                }
                crate::infra::fsio::write_bytes_atomic(&journal, &serde_json::to_vec_pretty(&doc)?)?;
            }
        }
    }
    Ok(Restored { snapshot, previous })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_utc_stamps() {
        assert_eq!(utc_stamp(0), "19700101-000000");
        assert_eq!(utc_stamp(1_700_000_000), "20231114-221320");
        assert_eq!(utc_stamp(951_782_400), "20000229-000000");
    }
}
