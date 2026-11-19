//! What opening a library with this build would do, without doing it (`toybox-server migrate --check`,
//! `tbx library check`).
//!
//! Nothing is written: the journal is read, the database is opened read-only and `immutable` (unless it
//! has a write-ahead log with changes in it), nothing is created, not even a folder.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::domain::library_migrations::{LATEST_VERSION, STEPS};
use crate::domain::sources;
use crate::error::AppResult;
use crate::infra::snapshot;

/// Roughly how fast step 5 copies and hashes, in bytes per second, and what it costs per item.
const BYTES_PER_SECOND: u64 = 120 * 1024 * 1024;
const SECONDS_PER_ITEM_MILLIS: u64 = 15;

/// One migration that has not run yet.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PendingStep {
    pub version: u32,
    pub name: String,
    /// What it does, in a sentence.
    pub summary: String,
    /// Items it would rewrite, when that can be counted.
    pub items: Option<u64>,
    /// Files it would move, when that can be counted.
    pub files: Option<u64>,
    /// Bytes of those files.
    pub bytes: Option<u64>,
    /// Whether it rewrites entries (a snapshot is taken first) .
    pub rewrites_entries: bool,
}

/// The report.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Preflight {
    pub root: String,
    /// The library format version it is at now (0: not migrated at all yet).
    pub format_version: u32,
    /// The version this build brings it to.
    pub latest_version: u32,
    /// Migrated by a newer Toybox than this build: it will be refused.
    pub newer_than_this_build: bool,
    pub pending: Vec<PendingStep>,
    /// A rough time for all of it, in seconds.
    pub estimate_seconds: Option<u64>,
    /// Free bytes on the library's disk.
    pub free_bytes: Option<u64>,
    /// Bytes it needs at its peak (the snapshot plus the files in flight).
    pub needed_bytes: u64,
    pub enough_space: bool,
    /// Where the snapshot will go, when one is taken.
    pub snapshot: Option<String>,
    /// After the migration, Toybox apps and servers older than this are refused.
    pub refused_before: Option<String>,
    pub notes: Vec<String>,
}

fn journal_version(root: &Path) -> AppResult<u32> {
    let path = root.join("_library_config/migrations.json");
    match std::fs::read(&path) {
        Ok(bytes) => {
            let journal: Value = serde_json::from_slice(&bytes)?;
            Ok(journal.get("version").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).unwrap_or(0))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(crate::error::AppError::file("Read", &path, error)),
    }
}

/// What step 5 would move, counted in the database that is there.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SourceCount {
    pub items: u64,
    pub files: u64,
    pub bytes: u64,
    /// The biggest 40 items' bytes: what is in flight at once.
    pub peak_bytes: u64,
    pub done: bool,
}

/// Count the kept source files that are not in shared storage yet, without changing the database.
pub fn count_unshared_sources(root: &Path) -> AppResult<SourceCount> {
    let db = root.join(crate::config::LIBRARY_DB_REL);
    let mut count = SourceCount::default();
    if !db.is_file() {
        return Ok(count);
    }
    let connection = snapshot::open_unchanged(&db)?;
    count.done = connection
        .query_row("SELECT value FROM meta WHERE key = 'sharedSources'", [], |row| row.get::<_, String>(0))
        .map(|value| value == "done")
        .unwrap_or(false);
    let mut statement = match connection.prepare("SELECT entry FROM items WHERE instr(entry, '\"origin\"') > 0") {
        Ok(statement) => statement,
        Err(_) => return Ok(count),
    };
    let mut rows = statement.query([])?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut per_item: Vec<u64> = Vec::new();
    let root_text = root.to_string_lossy().to_string();
    while let Some(row) = rows.next()? {
        let text: String = row.get(0)?;
        let Ok(entry) = serde_json::from_str::<Value>(&text) else { continue };
        let mut item_bytes = 0;
        let mut any = false;
        for item in std::iter::once(&entry).chain(entry["variants"].as_array().into_iter().flatten()) {
            let folder = item["storagePath"].as_str().or_else(|| entry["storagePath"].as_str()).unwrap_or("");
            for file in item["origin"]["files"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if sources::is_shared(file) {
                    continue;
                }
                any = true;
                // A bare name lives in the item's own folder.
                let relative = if file.contains('/') || folder.is_empty() { file.to_string() } else { format!("{folder}/{file}") };
                if seen.insert(relative.clone()) {
                    count.files += 1;
                    let size = std::fs::metadata(crate::domain::paths::abs_under_root(&root_text, &relative)).map(|m| m.len()).unwrap_or(0);
                    count.bytes += size;
                    item_bytes += size;
                }
            }
        }
        if any {
            count.items += 1;
            per_item.push(item_bytes);
        }
    }
    per_item.sort_unstable_by(|a, b| b.cmp(a));
    count.peak_bytes = per_item.iter().take(40).sum();
    Ok(count)
}

fn summary(version: u32) -> &'static str {
    match version {
        1 => "mark every entry's kind explicitly",
        2 => "record where each entry came from (source, publisher, licence)",
        3 => "move media into typed storage folders",
        4 => "move the manifest, the cache and the content hashes into databases",
        5 => "store each kept source file once per library, shared between items",
        _ => "update the library format",
    }
}

/// Look at the library at `root` and say what opening it with this build would do.
pub fn check(root: &Path) -> AppResult<Preflight> {
    let format_version = journal_version(root)?;
    let db = root.join(crate::config::LIBRARY_DB_REL);
    let mut notes = Vec::new();
    let newer = format_version > LATEST_VERSION;
    let mut count = SourceCount::default();
    let mut counted = false;
    if db.is_file() {
        count = count_unshared_sources(root)?;
        counted = true;
    }
    let mut pending = Vec::new();
    if !newer {
        for &(version, name) in STEPS.iter().filter(|(v, _)| *v > format_version) {
            let mut step = PendingStep { version, name: name.to_string(), summary: summary(version).to_string(), items: None, files: None, bytes: None, rewrites_entries: version >= 4 };
            if version == 5 {
                if counted {
                    step.items = Some(count.items);
                    step.files = Some(count.files);
                    step.bytes = Some(count.bytes);
                    step.rewrites_entries = count.items > 0;
                } else {
                    notes.push("Kept source files are counted once the library is in a database (step 4).".into());
                }
            }
            pending.push(step);
        }
        // Step 5 started (the journal says 5) but did not finish: it resumes at the next open.
        if format_version == 5 && counted && !count.done {
            pending.push(PendingStep { version: 5, name: "shared-sources".into(), summary: "finish storing each kept source file once (an earlier run was interrupted)".into(), items: Some(count.items), files: Some(count.files), bytes: Some(count.bytes), rewrites_entries: false });
        }
    } else {
        notes.push(format!("This library was migrated by a newer Toybox (format {format_version}; this build knows {LATEST_VERSION}). It will be refused until Toybox is updated."));
    }
    let snapshot_wanted = pending.iter().any(|step| step.version == 5 && step.rewrites_entries && format_version < 5);
    let db_bytes = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0);
    let needed_bytes = if snapshot_wanted { db_bytes + count.peak_bytes + 64 * 1024 * 1024 } else if pending.is_empty() { 0 } else { 64 * 1024 * 1024 };
    let free_bytes = fs4::available_space(if root.exists() { root } else { Path::new(".") }).ok();
    let estimate_seconds = if pending.is_empty() {
        None
    } else {
        Some(count.bytes / BYTES_PER_SECOND + count.items * SECONDS_PER_ITEM_MILLIS / 1000 + 1)
    };
    let enough_space = free_bytes.map(|free| free >= needed_bytes).unwrap_or(true);
    if !enough_space {
        notes.push("There is not enough free disk space for this migration.".into());
    }
    if pending.iter().any(|step| step.version == 5) {
        notes.push(format!(
            "Afterwards, Toybox apps and servers older than {} refuse this library; keep the snapshot and the previous server binary until you are sure.",
            env!("CARGO_PKG_VERSION")
        ));
    }
    let refused_before = if pending.is_empty() { None } else { Some(env!("CARGO_PKG_VERSION").to_string()) };
    Ok(Preflight {
        root: root.to_string_lossy().to_string(),
        format_version,
        latest_version: LATEST_VERSION,
        newer_than_this_build: newer,
        pending,
        estimate_seconds,
        free_bytes,
        needed_bytes,
        enough_space,
        snapshot: snapshot_wanted.then(|| snapshot::planned_path(root, 4).to_string_lossy().to_string()),
        refused_before,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_library_reports_everything_pending_and_creates_nothing() {
        let root = std::env::temp_dir().join(format!("toybox-preflight-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let report = check(&root).unwrap();
        assert_eq!(report.format_version, 0);
        assert_eq!(report.pending.len(), 5);
        assert!(!root.exists());
    }
}
