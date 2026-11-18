//! Bounded retention for safety copies, and removal of empty set folders.
//!
//! Every publication, delete and metadata edit writes a "before" copy of the
//! catalogs next to the library. Those copies are insurance against the
//! operation that just ran, not history, so only the newest few are kept per
//! location. Nothing here may fail or delay the operation that triggered it:
//! every function swallows I/O errors and a missing folder is not an error.
//!
//! Policy (one place, so docs and code agree):
//! - `_library_config/migration-backups/storage-<nanos>/` (one per storage
//!   publication): the newest [`KEEP_STORAGE_SNAPSHOTS`] are kept.
//! - `catalog-before-delete-<nanos>.json`: the newest [`KEEP_DELETE_CATALOGS`]
//!   are kept in each of the two places a delete writes one (the library's
//!   `_library_config/` and the folder holding the catalog cache).
//! - `migration-backups/<NNNN-ts>/` (one per schema migration) is never pruned.
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub const KEEP_STORAGE_SNAPSHOTS: usize = 5;
pub const KEEP_DELETE_CATALOGS: usize = 5;
pub const STORAGE_SNAPSHOT_PREFIX: &str = "storage-";
pub const DELETE_CATALOG_PREFIX: &str = "catalog-before-delete-";

/// The timestamp between `prefix` and `suffix`, only if that is all digits.
fn stamp_of(name: &str, prefix: &str, suffix: &str) -> Option<u128> {
    let middle = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
    if middle.is_empty() || !middle.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    middle.parse().ok()
}

/// Keep the newest `keep` entries of `dir` named `<prefix><digits><suffix>`
/// (newest by the number in the name) and remove the rest, files or folders.
///
/// Entries whose name does not match exactly, symbolic links, and entries for
/// which `protected(name)` is true are never touched and do not count towards
/// `keep`. Returns how many entries were removed. Any error (a missing `dir`,
/// an entry that is gone already, a file another process holds open) is
/// ignored: pruning is best effort and retried on the next occasion.
pub fn prune_newest(dir: &Path, prefix: &str, suffix: &str, keep: usize, protected: &dyn Fn(&str) -> bool) -> usize {
    let Ok(read) = std::fs::read_dir(dir) else { return 0 };
    let mut found: Vec<(u128, String, bool)> = Vec::new();
    for entry in read.flatten() {
        let Some(name) = entry.file_name().to_str().map(String::from) else { continue };
        let Some(stamp) = stamp_of(&name, prefix, suffix) else { continue };
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() || protected(&name) {
            continue;
        }
        found.push((stamp, name, kind.is_dir()));
    }
    found.sort();
    let extra = found.len().saturating_sub(keep);
    let mut removed = 0;
    for (_, name, is_dir) in found.into_iter().take(extra) {
        let path = dir.join(&name);
        let gone = if is_dir {
            std::fs::remove_dir_all(&path).is_ok()
        } else {
            crate::infra::fsio::remove_file_if_exists(&path).is_ok()
        };
        removed += usize::from(gone);
    }
    removed
}

/// Keep the newest [`KEEP_DELETE_CATALOGS`] `catalog-before-delete-*.json` of `dir`.
pub fn prune_delete_catalogs(dir: &Path) -> usize {
    prune_newest(dir, DELETE_CATALOG_PREFIX, ".json", KEEP_DELETE_CATALOGS, &|_| false)
}

/// A content-addressed set folder is named by the hex SHA-256 of its identity (the
/// storage path check also accepts the 16-digit form of older versions).
pub fn is_set_folder_name(name: &str) -> bool {
    matches!(name.len(), 16 | 64) && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Remove the set folder (`library/<type>/<key>`) that held `file`, if that
/// leaves it empty. Only a directory named like a set key, directly inside a
/// type folder directly inside a folder named `library`, is ever considered,
/// and only with `remove_dir`, which refuses a folder that has any content.
pub fn remove_set_folder_if_empty(file: &Path) {
    let Some(set) = file.parent() else { return };
    let Some(kind) = set.parent() else { return };
    let (Some(set_name), Some(kind_name)) = (set.file_name().and_then(|n| n.to_str()), kind.file_name().and_then(|n| n.to_str())) else { return };
    if !is_set_folder_name(set_name) || kind_name.starts_with('_') || kind.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) != Some("library") {
        return;
    }
    let _ = std::fs::remove_dir(set);
}

/// Remove every EMPTY set folder under `<root>/library/<type>/`: a directory
/// named like a set key that contains nothing at all. Costs one directory
/// listing per type folder plus one per candidate; file contents are never
/// read and nothing else (non-empty folders, `_resources`, `_import_*`, the
/// type folders themselves, symbolic links) is touched. Returns the count.
pub fn sweep_empty_set_folders(root: &Path) -> usize {
    let Ok(types) = std::fs::read_dir(root.join("library")) else { return 0 };
    let mut removed = 0;
    for kind in types.flatten() {
        let name = kind.file_name();
        if name.to_str().is_none_or(|n| n.starts_with('_')) || !kind.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(sets) = std::fs::read_dir(kind.path()) else { continue };
        for set in sets.flatten() {
            if set.file_name().to_str().is_some_and(is_set_folder_name) && set.file_type().is_ok_and(|t| t.is_dir()) {
                removed += usize::from(std::fs::remove_dir(set.path()).is_ok());
            }
        }
    }
    removed
}

static SWEPT: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// The once-per-process housekeeping of an opened library: shrink piles of
/// safety copies left by earlier versions and remove empty set folders.
/// `cache` is the catalog cache file (its folder also receives delete copies).
/// Never fails; skipped (and retried at the next open) while a storage
/// publication is running.
pub fn on_open(root: &Path, cache: &Path) {
    let key = root.to_path_buf();
    if SWEPT.lock().ok().is_some_and(|seen| seen.as_ref().is_some_and(|set| set.contains(&key))) {
        return;
    }
    if housekeeping(root, cache) {
        if let Ok(mut seen) = SWEPT.lock() {
            seen.get_or_insert_with(HashSet::new).insert(key);
        }
    }
}

/// One housekeeping pass; false when it could not run (a publication is in progress).
pub fn housekeeping(root: &Path, cache: &Path) -> bool {
    prune_delete_catalogs(&root.join("_library_config"));
    if let Some(folder) = cache.parent() {
        prune_delete_catalogs(folder);
    }
    crate::infra::storage::tidy(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("toybox-retention-{name}-{}-{}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn names(dir: &Path) -> Vec<String> {
        let mut all: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        all.sort();
        all
    }
    const KEY_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const KEY_B: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn keeps_the_newest_by_stamp_not_by_text_and_leaves_other_names_alone() {
        let dir = fixture("order");
        // 9 < 10 < 100 numerically; as text "9" would sort last.
        for stamp in ["9", "10", "100", "1000"] {
            std::fs::write(dir.join(format!("catalog-before-delete-{stamp}.json")), b"x").unwrap();
        }
        for other in ["catalog-before-delete-.json", "catalog-before-delete-abc.json", "catalog-before-delete-5.json.bak", "catalog-before-delete-7.tmp", "catalog.json", "catalog-before-metadata-1.json"] {
            std::fs::write(dir.join(other), b"x").unwrap();
        }
        std::fs::create_dir(dir.join("catalog-before-delete-1.json")).unwrap(); // a folder of that name is an entry too
        assert_eq!(prune_newest(&dir, DELETE_CATALOG_PREFIX, ".json", 2, &|_| false), 3);
        let left = names(&dir);
        assert!(left.contains(&"catalog-before-delete-100.json".into()) && left.contains(&"catalog-before-delete-1000.json".into()));
        assert!(!left.contains(&"catalog-before-delete-9.json".into()) && !left.contains(&"catalog-before-delete-10.json".into()));
        assert!(!left.contains(&"catalog-before-delete-1.json".into()));
        for other in ["catalog-before-delete-.json", "catalog-before-delete-abc.json", "catalog-before-delete-5.json.bak", "catalog-before-delete-7.tmp", "catalog.json", "catalog-before-metadata-1.json"] {
            assert!(left.contains(&other.to_string()), "{other} was touched");
        }
        assert_eq!(prune_newest(&dir, DELETE_CATALOG_PREFIX, ".json", 2, &|_| false), 0, "idempotent");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn removes_whole_snapshot_folders_but_skips_protected_ones() {
        let dir = fixture("folders");
        for stamp in 1..=7 {
            std::fs::create_dir_all(dir.join(format!("storage-{stamp}/unindexed"))).unwrap();
            std::fs::write(dir.join(format!("storage-{stamp}/transaction.json")), b"x").unwrap();
        }
        std::fs::create_dir(dir.join("0001-123")).unwrap(); // a schema migration backup
        std::fs::create_dir(dir.join("storage-test")).unwrap(); // not a stamp
        let removed = prune_newest(&dir, STORAGE_SNAPSHOT_PREFIX, "", 3, &|name| name == "storage-1");
        assert_eq!(removed, 3); // 2,3,4 go; 1 is protected and not counted; 5,6,7 stay
        assert_eq!(names(&dir), ["0001-123", "storage-1", "storage-5", "storage-6", "storage-7", "storage-test"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_folder_or_a_file_in_its_place_is_not_an_error() {
        let dir = fixture("missing");
        assert_eq!(prune_newest(&dir.join("absent"), "a-", "", 1, &|_| false), 0);
        std::fs::write(dir.join("file"), b"x").unwrap();
        assert_eq!(prune_newest(&dir.join("file"), "a-", "", 1, &|_| false), 0);
        assert_eq!(prune_delete_catalogs(&dir.join("absent")), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_entry_that_cannot_be_removed_is_skipped_and_the_rest_still_go() {
        let dir = fixture("errors");
        for stamp in 1..=4 {
            std::fs::write(dir.join(format!("x-{stamp}.json")), b"x").unwrap();
        }
        // Hold the oldest open without sharing deletes where the OS enforces it.
        #[cfg(windows)]
        let _held = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new().read(true).share_mode(1).open(dir.join("x-1.json")).unwrap()
        };
        let removed = prune_newest(&dir, "x-", ".json", 1, &|_| false);
        assert!(dir.join("x-4.json").exists());
        assert!(!dir.join("x-2.json").exists() && !dir.join("x-3.json").exists());
        assert_eq!(dir.join("x-1.json").exists(), cfg!(windows));
        assert_eq!(removed, if cfg!(windows) { 2 } else { 3 });
        // Concurrent deletion by someone else is also fine: nothing left to remove.
        std::fs::remove_dir_all(&dir).unwrap_or_default();
        assert_eq!(prune_newest(&dir, "x-", ".json", 1, &|_| false), 0);
    }

    #[test]
    fn only_empty_set_folders_are_swept() {
        let root = fixture("sweep");
        let library = root.join("library");
        for kind in ["audio", "models", "my.plugin"] {
            std::fs::create_dir_all(library.join(kind).join(KEY_A)).unwrap();
        }
        std::fs::create_dir_all(library.join("audio").join(KEY_B)).unwrap();
        std::fs::write(library.join("audio").join(KEY_B).join("stray.wav"), b"x").unwrap();
        std::fs::create_dir_all(library.join("audio/not-a-key")).unwrap();
        std::fs::create_dir_all(library.join("audio").join("0123456789abcdef")).unwrap();
        std::fs::create_dir_all(library.join("audio").join(&KEY_A[1..])).unwrap(); // 63 digits
        std::fs::create_dir_all(library.join("audio").join(KEY_A.replace('a', "g"))).unwrap(); // not hex
        std::fs::create_dir_all(library.join("models/_resources")).unwrap();
        std::fs::create_dir_all(library.join("_import_stage").join(KEY_A)).unwrap();
        std::fs::create_dir_all(library.join("audio").join(KEY_B).join("nested").join(KEY_A)).unwrap();
        std::fs::write(library.join("audio").join(KEY_A.replace('a', "b")), b"a file named like a key").unwrap();
        let removed = sweep_empty_set_folders(&root);
        assert_eq!(removed, 4); // audio/KEY_A, audio/<16 digits>, models/KEY_A, my.plugin/KEY_A
        assert!(!library.join("audio/0123456789abcdef").exists());
        assert!(library.join("audio").join(&KEY_A[1..]).is_dir() && library.join("audio").join(KEY_A.replace('a', "g")).is_dir());
        assert!(!library.join("models").join(KEY_A).exists() && !library.join("my.plugin").join(KEY_A).exists());
        assert!(library.join("audio").is_dir() && library.join("models/_resources").is_dir());
        assert!(library.join("audio").join(KEY_B).join("stray.wav").exists(), "a folder with a stray file is left");
        assert!(library.join("audio").join(KEY_B).join("nested").join(KEY_A).exists(), "only the top level is considered");
        assert!(library.join("audio/not-a-key").is_dir());
        assert!(library.join("_import_stage").join(KEY_A).is_dir(), "staging is handled elsewhere");
        assert!(library.join("audio").join(KEY_A.replace('a', "b")).is_file());
        assert_eq!(sweep_empty_set_folders(&root.join("absent")), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_deleted_files_set_folder_goes_only_when_empty_and_only_when_shaped_like_one() {
        let root = fixture("setfolder");
        let set = root.join("library/audio").join(KEY_A);
        std::fs::create_dir_all(&set).unwrap();
        std::fs::write(set.join("a.wav"), b"x").unwrap();
        std::fs::write(set.join("b.wav"), b"x").unwrap();
        std::fs::remove_file(set.join("a.wav")).unwrap();
        remove_set_folder_if_empty(&set.join("a.wav"));
        assert!(set.is_dir(), "still holds b.wav");
        std::fs::remove_file(set.join("b.wav")).unwrap();
        remove_set_folder_if_empty(&set.join("b.wav"));
        assert!(!set.exists());
        assert!(root.join("library/audio").is_dir(), "the type folder stays");
        // Not a set folder: a resources folder, a legacy pack layout, a path outside `library`.
        for relative in ["library/models/_resources", "library/p/materials/one", "elsewhere/audio"] {
            let folder = root.join(relative);
            std::fs::create_dir_all(&folder).unwrap();
            remove_set_folder_if_empty(&folder.join("f.png"));
            assert!(folder.is_dir(), "{relative}");
        }
        let outside = root.join("elsewhere/audio").join(KEY_A);
        std::fs::create_dir_all(&outside).unwrap();
        remove_set_folder_if_empty(&outside.join("f.wav"));
        assert!(outside.is_dir(), "not under a folder named library");
        let hidden = root.join("library/_resources").join(KEY_A);
        std::fs::create_dir_all(&hidden).unwrap();
        remove_set_folder_if_empty(&hidden.join("f.wav"));
        assert!(hidden.is_dir(), "underscore folders are not type folders");
        std::fs::remove_dir_all(root).unwrap();
    }
}
