//! Durable library upgrades, independent of derived catalog schema versions.
//! Every changed document is backed up; interrupted transactions roll back before retry.
//!
//! Step 4 (`sqlite-store`) moves the manifest, the cache and the content hashes
//! into databases (`infra::store::adopt`). A library that is still JSON is
//! adopted first, whatever step it is at, so steps 1 to 3 (and the storage
//! publication of step 3) always work on the database form; each of them
//! commits its documents in one database transaction after saving the originals
//! and the replacements beside it, and is idempotent, so a crash simply repeats it.
use crate::{
    config,
    domain::{catalog_migrate, catalog_model::Catalog},
    domain::library_migrations::{self, LATEST_VERSION, STEPS},
    error::{AppError, AppResult},
    infra::{fsio, store::{self, CatalogWrite, Store}},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

static LOCK: Mutex<()> = Mutex::new(());
const JOURNAL: &str = "migrations.json";
const PENDING: &str = "migration-pending.json";

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Slot {
    Manifest,
    Cache,
    Journal,
}
impl Slot {
    fn name(self) -> &'static str {
        match self {
            Self::Manifest => "manifest",
            Self::Cache => "cache",
            Self::Journal => "journal",
        }
    }
    fn path(self, root: &Path, cache: &Path) -> PathBuf {
        match self {
            Self::Manifest => root.join(config::SEED_REL),
            Self::Cache => cache.into(),
            Self::Journal => root.join("_library_config").join(JOURNAL),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    slot: Slot,
    before_exists: bool,
}
#[derive(Serialize, Deserialize)]
struct Pending {
    directory: String,
    version: u32,
    files: Vec<Snapshot>,
}

fn read_optional(path: &Path) -> AppResult<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::file("Read", path, error)),
    }
}
fn encoded(doc: &Value) -> AppResult<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(doc)?)
}
fn version(journal: &Value) -> AppResult<u32> {
    journal
        .get("version")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| AppError::msg("Invalid migration journal version"))
}

/// Called before catalog loading/rebuilding. Step 3 relocates media through the
/// separate verified storage transaction; it does not derive catalog facts.
pub fn run(root: &Path, cache: &Path) -> AppResult<()> {
    let _lock = LOCK
        .lock()
        .map_err(|_| AppError::msg("Migration lock was poisoned"))?;
    let journal_path = root.join("_library_config").join(JOURNAL);
    let mut journal: Value = match read_optional(&journal_path)? {
        Some(bytes) => serde_json::from_slice(&bytes)?,
        None => json!({"version":0,"applied":[]}),
    };
    let current = version(&journal)?;
    // A library a newer Toybox migrated is refused before anything is touched.
    if current > LATEST_VERSION {
        return Err(AppError::msg(
            "This library was migrated by a newer Toybox. Install that version or newer.",
        ));
    }
    // An interrupted JSON migration of an older Toybox is rolled back first; then
    // the JSON files (if any) become databases; then an interrupted publication finishes.
    recover(root, cache)?;
    store::adopt::adopt_local(root, cache)?;
    crate::infra::storage::recover(&root.canonicalize()?, cache)?;
    crate::infra::retention::on_open(root, cache);
    if current >= 5 {
        // Step 5 moves files and can be interrupted after its journal entry: finish it.
        finish_shared_sources(root, cache)?;
    }
    if current == LATEST_VERSION {
        return Ok(());
    }
    for &(next, name) in STEPS.iter().filter(|(next, _)| *next > current) {
        if next == 5 {
            // The journal first: from here on an older Toybox refuses the library, so it never reads the
            // new layout. Then the files move; an interruption is finished at the next open.
            let store = Store::local(root, cache);
            journal["version"] = json!(next);
            journal["applied"]
                .as_array_mut()
                .ok_or_else(|| AppError::msg("Invalid migration journal history"))?
                .push(json!({"version":next,"name":name,"appVersion":env!("CARGO_PKG_VERSION")}));
            commit(
                root,
                &store,
                next,
                Documents { manifest: (Value::Null, Value::Null), cache: None, journal_before: read_optional(&journal_path)?, journal_after: encoded(&journal)? },
            )?;
            finish_shared_sources(root, cache)?;
            continue;
        }
        if next == 3 { crate::infra::storage::migrate(root, cache)?; }
        let store = Store::local(root, cache);
        let before = store::read_manifest(root)?;
        let mut manifest = before.clone();
        if !manifest["assets"].is_array() {
            return Err(AppError::msg(
                "Cannot migrate library: catalog assets must be an array",
            ));
        }
        let cached_catalog = store.load_catalog()?;
        let cached: Option<Value> = cached_catalog.as_ref().map(serde_json::to_value).transpose()?;
        let mut cached_doc = cached.clone();
        let packs = if next == 2 {
            read_pack_metadata(root, &manifest, cached_doc.as_ref())?
        } else {
            HashMap::new()
        };
        let from_cache = cached_doc
            .as_ref()
            .map(|doc| library_migrations::provenance_index(doc, true))
            .unwrap_or_default();
        let from_manifest = library_migrations::provenance_index(&manifest, false);
        if next == 2 {
            if let Some((id, field)) = library_migrations::conflict(&from_manifest, &from_cache) {
                return Err(AppError::msg(format!("Migration stopped: conflicting explicit {field} values for asset '{id}' in the manifest and cached user metadata. Resolve those values and retry.")));
            }
        }
        library_migrations::apply(&mut manifest, next, false, &packs, &from_cache);
        if let Some(doc) = &mut cached_doc {
            library_migrations::apply(doc, next, true, &packs, &from_manifest);
        }
        journal["version"] = json!(next);
        if !journal["applied"].is_array() {
            return Err(AppError::msg("Invalid migration journal history"));
        }
        journal["applied"]
            .as_array_mut()
            .unwrap()
            .push(json!({"version":next,"name":name,"appVersion":env!("CARGO_PKG_VERSION")}));
        let catalog_after: Option<Catalog> = cached_doc.as_ref().map(|doc| serde_json::from_value(catalog_migrate::upgrade(doc.clone()))).transpose()?;
        commit(
            root,
            &store,
            next,
            Documents { manifest: (before, manifest), cache: cached.zip(cached_doc).zip(catalog_after) .map(|((b, a), c)| (b, a, c)), journal_before: read_optional(&journal_path)?, journal_after: encoded(&journal)? },
        )?;
    }
    Ok(())
}

/// Move the kept source files into shared storage unless that is done. Says what it did on standard error.
fn finish_shared_sources(root: &Path, cache: &Path) -> AppResult<()> {
    let store = Store::local(root, cache);
    if crate::infra::source_migration::is_done(&store) {
        return Ok(());
    }
    let started = std::time::Instant::now();
    let report = crate::infra::source_migration::migrate(root, cache)?;
    if report.items > 0 || !report.skipped.is_empty() {
        eprintln!(
            "toybox: kept source files are stored once per library now: {} items, {} files, {} MB freed in {:.1}s{}",
            report.items,
            report.shared_files,
            report.bytes_freed / (1024 * 1024),
            started.elapsed().as_secs_f64(),
            if report.skipped.is_empty() { String::new() } else { format!(" ({} items could not be converted and keep their files)", report.skipped.len()) }
        );
    }
    Ok(())
}

fn read_pack_metadata(
    root: &Path,
    manifest: &Value,
    cache: Option<&Value>,
) -> AppResult<HashMap<String, Value>> {
    fn packs(doc: &Value, found: &mut HashSet<String>) {
        if let Some(entries) = doc["assets"].as_array() {
            for entry in entries {
                if let Some(pack) = entry["pack"].as_str() {
                    found.insert(pack.into());
                }
                if let Some(variants) = entry["variants"].as_array() {
                    packs(&json!({"assets":variants}), found);
                }
            }
        }
    }
    let mut names = HashSet::new();
    packs(manifest, &mut names);
    if let Some(cache) = cache {
        packs(cache, &mut names);
    }
    let canonical_root = root.canonicalize()?;
    let mut metadata = HashMap::new();
    for name in names {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) {
            continue;
        }
        let path = root.join("library").join(&name).join("pack.json");
        if !path.exists() {
            continue;
        }
        if !path.canonicalize()?.starts_with(&canonical_root) {
            return Err(AppError::msg("Pack metadata is outside the library"));
        }
        metadata.insert(name, serde_json::from_slice(&fsio::read_bytes(&path)?)?);
    }
    Ok(metadata)
}

/// What one step replaces: the manifest, the catalog (as the document it was and
/// the document it becomes) and the journal.
struct Documents {
    manifest: (Value, Value),
    cache: Option<(Value, Value, Catalog)>,
    journal_before: Option<Vec<u8>>,
    journal_after: Vec<u8>,
}

/// Save the originals and the replacements beside the library (the same
/// `<version>-<stamp>/<slot>.before.json` / `.after.json` files as ever), then
/// commit the manifest and the catalog in one database transaction, then the
/// journal. The transaction is refused if the manifest moved meanwhile. A crash
/// before the journal is written repeats the step, which changes nothing twice.
fn commit(root: &Path, store: &Store, version: u32, documents: Documents) -> AppResult<()> {
    let manifest_changes = documents.manifest.0 != documents.manifest.1;
    let cache_changes = documents.cache.as_ref().is_some_and(|(before, after, _)| before != after);
    let journal_changes = documents.journal_before.as_ref() != Some(&documents.journal_after);
    if !manifest_changes && !cache_changes && !journal_changes {
        return Ok(());
    }
    let directory = format!(
        "{version:04}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AppError::msg("Invalid system time"))?
            .as_nanos()
    );
    let config = root.join("_library_config");
    let backup = config.join("migration-backups").join(&directory);
    let save = |slot: Slot, before: Option<Vec<u8>>, after: Vec<u8>| -> AppResult<()> {
        if let Some(bytes) = before {
            fsio::write_bytes(&backup.join(format!("{}.before.json", slot.name())), &bytes)?;
        }
        fsio::write_bytes(&backup.join(format!("{}.after.json", slot.name())), &after)
    };
    if manifest_changes {
        save(Slot::Manifest, Some(encoded(&documents.manifest.0)?), encoded(&documents.manifest.1)?)?;
    }
    if cache_changes {
        if let Some((before, after, _)) = &documents.cache {
            save(Slot::Cache, Some(encoded(before)?), encoded(after)?)?;
        }
    }
    if journal_changes {
        save(Slot::Journal, documents.journal_before.clone(), documents.journal_after.clone())?;
    }
    let (_, revision) = store::read_manifest_with_revision(root)?;
    if manifest_changes || cache_changes {
        store.commit(CatalogWrite {
            manifest: Some(&documents.manifest.1),
            catalog: if cache_changes { documents.cache.as_ref().map(|(_, _, catalog)| catalog) } else { None },
            expect_manifest_revision: Some(revision),
        })?;
    }
    if journal_changes {
        fsio::write_bytes_atomic(&config.join(JOURNAL), &documents.journal_after)?;
    }
    Ok(())
}

fn recover(root: &Path, cache: &Path) -> AppResult<()> {
    let config = root.join("_library_config");
    let Some(bytes) = read_optional(&config.join(PENDING))? else {
        return Ok(());
    };
    let pending: Pending = serde_json::from_slice(&bytes)?;
    if pending.version > LATEST_VERSION
        || pending.directory.is_empty()
        || !pending
            .directory
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        return Err(AppError::msg(
            "Unsupported or invalid pending library migration",
        ));
    }
    let backup = config.join("migration-backups").join(pending.directory);
    let mut restore = Vec::new();
    // Do not overwrite external edits made after an interrupted migration.
    for file in pending.files {
        let path = file.slot.path(root, cache);
        let before = if file.before_exists {
            Some(fsio::read_bytes(
                &backup.join(format!("{}.before.json", file.slot.name())),
            )?)
        } else {
            None
        };
        let after = fsio::read_bytes(&backup.join(format!("{}.after.json", file.slot.name())))?;
        let current = read_optional(&path)?;
        if current != before && current.as_ref() != Some(&after) {
            return Err(AppError::msg(format!(
                "Migration recovery stopped: '{}' was edited externally. Backups: '{}'",
                path.display(),
                backup.display()
            )));
        }
        restore.push((path, before));
    }
    for (path, before) in restore {
        if let Some(bytes) = before {
            fsio::write_bytes_atomic(&path, &bytes)?;
        } else {
            fsio::remove_file_if_exists(&path)?;
        }
    }
    fsio::remove_file_if_exists(&config.join(PENDING))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> (PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("toybox-migrate-{name}-{}", std::process::id()));
        std::fs::create_dir_all(root.join("_library_config")).unwrap();
        let manifest = json!({"unknown":"keep","assets":[{"id":"a","pack":"original_pack","category":"props","file":"Thing.gltf"}]});
        fsio::write_bytes(&root.join(config::SEED_REL), &encoded(&manifest).unwrap()).unwrap();
        fsio::write_bytes(&root.join("library/original_pack/props/Thing.gltf"),br#"{"asset":{"version":"2.0"},"nodes":[],"buffers":[]}"#).unwrap();
        let cache = root.join("appdata/catalog.json");
        fsio::write_bytes(&cache,&encoded(&json!({"schemaVersion":5,"libraryRoot":root,"assets":[{"id":"a","name":"Thing","fileName":"Thing.gltf","relPath":"library/original_pack/props/Thing.gltf","pack":"original_pack","category":"props","kind":"model","fileset":{"gltf":"library/original_pack/props/Thing.gltf","bin":"library/original_pack/props/Thing.bin","textures":[]},"thumb":{"state":"missing"},"user":{"favorite":true,"tags":["custom"],"collections":["Game"]}}]})).unwrap()).unwrap();
        (root, cache)
    }
    #[test]
    fn upgrades_once_preserves_metadata_and_keeps_backups() {
        let (root, cache) = fixture("once");
        run(&root, &cache).unwrap();
        let doc = serde_json::to_value(Store::local(&root, &cache).load_catalog().unwrap().unwrap()).unwrap();
        assert_eq!(
            doc["assets"][0]["user"]["provenance"]["source"],
            "original_pack"
        );
        assert_eq!(doc["assets"][0]["user"]["favorite"], true);
        let manifest = store::read_manifest(&root).unwrap();
        assert_eq!(manifest["unknown"], "keep");
        let journal = fsio::read_bytes(&root.join("_library_config").join(JOURNAL)).unwrap();
        run(&root, &cache).unwrap();
        assert_eq!(store::read_manifest(&root).unwrap(), manifest);
        assert_eq!(
            fsio::read_bytes(&root.join("_library_config").join(JOURNAL)).unwrap(),
            journal
        );
        // The old files were moved aside, not deleted: steps 1, 2, the storage
        // snapshot of step 3, the files moved by step 4 and its journal record.
        assert!(!root.join(config::SEED_REL).exists() && !cache.exists());
        assert_eq!(
            std::fs::read_dir(root.join("_library_config/migration-backups"))
                .unwrap()
                .count(),
            6
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn future_version_refuses_downgrade_without_writing_data() {
        let (root, cache) = fixture("future");
        let before = fsio::read_bytes(&root.join(config::SEED_REL)).unwrap();
        fsio::write_bytes(
            &root.join("_library_config").join(JOURNAL),
            br#"{"version":999,"applied":[]}"#,
        )
        .unwrap();
        assert!(run(&root, &cache).is_err());
        assert_eq!(
            fsio::read_bytes(&root.join(config::SEED_REL)).unwrap(),
            before
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn interrupted_transaction_restores_originals_and_rejects_external_edits() {
        let (root, cache) = fixture("recovery");
        let path = root.join(config::SEED_REL);
        let before = fsio::read_bytes(&path).unwrap();
        let after = br#"{"assets":[],"changed":true}"#;
        let config = root.join("_library_config");
        let backup = config.join("migration-backups/0001-123");
        fsio::write_bytes(&backup.join("manifest.before.json"), &before).unwrap();
        fsio::write_bytes(&backup.join("manifest.after.json"), after).unwrap();
        let marker = serde_json::to_vec(&Pending {
            directory: "0001-123".into(),
            version: 1,
            files: vec![Snapshot {
                slot: Slot::Manifest,
                before_exists: true,
            }],
        })
        .unwrap();
        fsio::write_bytes(&config.join(PENDING), &marker).unwrap();
        fsio::write_bytes(&path, b"external edit").unwrap();
        assert!(recover(&root, &cache).is_err());
        assert_eq!(fsio::read_bytes(&path).unwrap(), b"external edit");
        fsio::write_bytes(&path, after).unwrap();
        recover(&root, &cache).unwrap();
        assert_eq!(fsio::read_bytes(&path).unwrap(), before);
        assert!(!config.join(PENDING).exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    mod step_four {
        use super::*;
        use crate::infra::registry::{LibraryEntry, LibraryHandle};
        use crate::infra::store::tests::{catalog, manifest_doc, rooted, Tmp};

        fn handle(tmp: &Tmp) -> LibraryHandle {
            std::fs::create_dir_all(tmp.root()).unwrap();
            LibraryHandle::local(&tmp.0.join("appdata"), &LibraryEntry::local("lib", "lib", &tmp.root().to_string_lossy()))
        }

        /// A library the way step 3 left it: manifest, cache, content hashes and the journal, all JSON.
        fn step_three(tmp: &Tmp, schema: u32) -> (LibraryHandle, Value, Value) {
            let handle = handle(tmp);
            let manifest = manifest_doc();
            fsio::write_bytes(&handle.root.join(config::SEED_REL), &encoded(&manifest).unwrap()).unwrap();
            let mut cached = serde_json::to_value(rooted(catalog(), &Store::for_handle(&handle))).unwrap();
            cached["schemaVersion"] = json!(schema);
            fsio::write_bytes(&handle.cache_path, &encoded(&cached).unwrap()).unwrap();
            fsio::write_bytes(&handle.cache_path.with_file_name("content-hashes.json"), br#"{"scheme":1,"hashes":{"skybox/other":"kept"}}"#).unwrap();
            let applied: Vec<Value> = STEPS.iter().filter(|(version, _)| *version <= 3).map(|(version, name)| json!({"version":version,"name":name,"appVersion":"0.4.1"})).collect();
            fsio::write_bytes(&handle.root.join("_library_config").join(JOURNAL), &encoded(&json!({"version":3,"applied":applied})).unwrap()).unwrap();
            (handle, manifest, cached)
        }

        #[test]
        fn a_step_three_library_becomes_databases_and_the_journal_says_so() {
            let tmp = Tmp::new("run-step3");
            let (handle, manifest, cached) = step_three(&tmp, config::SCHEMA_VERSION);
            run(&handle.root, &handle.cache_path).unwrap();
            let journal: Value = serde_json::from_slice(&fsio::read_bytes(&handle.root.join("_library_config").join(JOURNAL)).unwrap()).unwrap();
            assert_eq!(journal["version"], 5);
            assert_eq!(journal["applied"].as_array().unwrap().iter().find(|step| step["version"] == 4).unwrap()["name"], "sqlite-store");
            // What the files said, the databases say.
            assert_eq!(store::read_manifest(&handle.root).unwrap(), manifest);
            let back = Store::for_handle(&handle).load_catalog().unwrap().unwrap();
            let expected: Catalog = serde_json::from_value(cached).unwrap();
            assert_eq!(serde_json::to_value(&back).unwrap(), crate::infra::store::tests::kept(&expected));
            assert_eq!(store::hashes::load(&handle.library_db_path()).get("skybox/other").map(String::as_str), Some("kept"));
            // The old files were moved to the backup folder, not deleted and not left in place.
            assert!(!handle.root.join(config::SEED_REL).exists() && !handle.cache_path.exists() && !handle.cache_path.with_file_name("content-hashes.json").exists());
            let backup = std::fs::read_dir(handle.root.join("_library_config/migration-backups")).unwrap().flatten().map(|e| e.path()).find(|p| p.join("manifest.before.json").is_file()).expect("a backup folder with the manifest");
            assert!(backup.join("cache.before.json").is_file() && backup.join("content-hashes.before.json").is_file());
            // Running it again changes nothing.
            let (stamp, journal_bytes) = (Store::for_handle(&handle).stamp().unwrap(), fsio::read_bytes(&handle.root.join("_library_config").join(JOURNAL)).unwrap());
            run(&handle.root, &handle.cache_path).unwrap();
            assert_eq!(Store::for_handle(&handle).stamp().unwrap(), stamp);
            assert_eq!(fsio::read_bytes(&handle.root.join("_library_config").join(JOURNAL)).unwrap(), journal_bytes);
        }

        #[test]
        fn a_crash_between_the_database_and_the_journal_is_finished_by_the_next_run() {
            let tmp = Tmp::new("run-crash");
            let (handle, manifest, _) = step_three(&tmp, config::SCHEMA_VERSION);
            // Crash while building: a leftover library.db.new.
            fsio::write_bytes(&handle.root.join("_library_config/library.db.new"), b"torn").unwrap();
            run(&handle.root, &handle.cache_path).unwrap();
            assert_eq!(store::read_manifest(&handle.root).unwrap(), manifest);
            assert!(!handle.root.join("_library_config/library.db.new").exists());
            // Crash after the database was installed and the files moved, before the journal: put the journal back to step 3.
            let journal_path = handle.root.join("_library_config").join(JOURNAL);
            let mut journal: Value = serde_json::from_slice(&fsio::read_bytes(&journal_path).unwrap()).unwrap();
            journal["version"] = json!(3);
            journal["applied"].as_array_mut().unwrap().truncate(3);
            fsio::write_bytes(&journal_path, &encoded(&journal).unwrap()).unwrap();
            let before = Store::for_handle(&handle).stamp().unwrap();
            run(&handle.root, &handle.cache_path).unwrap();
            let again: Value = serde_json::from_slice(&fsio::read_bytes(&journal_path).unwrap()).unwrap();
            assert_eq!(again["version"], 5);
            assert_eq!(Store::for_handle(&handle).stamp().unwrap(), before, "the databases were not touched again");
            assert_eq!(store::read_manifest(&handle.root).unwrap(), manifest);
        }

        #[test]
        fn a_cache_of_an_older_schema_is_kept_for_its_user_data_but_not_used() {
            let tmp = Tmp::new("run-stale");
            let (handle, _, _) = step_three(&tmp, config::SCHEMA_VERSION - 2);
            run(&handle.root, &handle.cache_path).unwrap();
            let back = Store::for_handle(&handle).load_catalog().unwrap().unwrap();
            assert_eq!(back.schema_version, config::SCHEMA_VERSION - 2);
            assert!(back.assets[0].user.favorite && back.assets[0].user.collections == vec!["Zoo".to_string(), "Alpha".to_string()]);
            assert!(crate::ops::catalog::load_current_catalog(&handle).unwrap().is_none(), "the app rescans, which merges the user data back in");
        }

        #[test]
        fn a_library_without_a_matching_cache_is_left_for_a_scan() {
            let tmp = Tmp::new("run-nocache");
            let (handle, manifest, _) = step_three(&tmp, config::SCHEMA_VERSION);
            // The cache in app data belongs to another folder.
            let mut foreign: Value = serde_json::from_slice(&fsio::read_bytes(&handle.cache_path).unwrap()).unwrap();
            foreign["libraryRoot"] = json!("Z:/elsewhere");
            fsio::write_bytes(&handle.cache_path, &encoded(&foreign).unwrap()).unwrap();
            run(&handle.root, &handle.cache_path).unwrap();
            assert_eq!(store::read_manifest(&handle.root).unwrap(), manifest);
            assert!(crate::ops::catalog::load_catalog_inner(&handle).unwrap().is_none());
            assert!(!handle.cache_path.exists(), "even that cache is kept aside, not deleted");
        }

        #[test]
        fn a_new_library_starts_as_a_database_and_never_has_the_json_files() {
            let tmp = Tmp::new("run-new");
            let root = tmp.root();
            crate::infra::library::ensure_library_folder(&root).unwrap();
            assert!(root.join(config::LIBRARY_DB_REL).is_file() && !root.join(config::SEED_REL).exists());
            crate::infra::library::validate(&root).unwrap();
            assert!(crate::infra::library::is_writable(&root));
            let cache = tmp.cache();
            run(&root, &cache).unwrap();
            let journal: Value = serde_json::from_slice(&fsio::read_bytes(&root.join("_library_config").join(JOURNAL)).unwrap()).unwrap();
            assert_eq!(journal["version"], 5);
            assert!(!root.join(config::SEED_REL).exists());
            assert_eq!(store::read_manifest(&root).unwrap()["assets"], json!([]));
            assert!(!std::fs::read_dir(root.join("_library_config/migration-backups")).map(|read| read.flatten().any(|e| e.path().join("manifest.before.json").exists())).unwrap_or(false), "there was nothing to move");
        }
    }
}
