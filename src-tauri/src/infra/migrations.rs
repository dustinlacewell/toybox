//! Durable library upgrades, independent of derived catalog schema versions.
//! Every changed document is backed up; interrupted transactions roll back before retry.
use crate::{
    config,
    domain::library_migrations::{self, LATEST_VERSION, STEPS},
    error::{AppError, AppResult},
    infra::fsio,
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

/// Called before any catalog is read or rebuilt; it never rewrites media files.
pub fn run(root: &Path, cache: &Path) -> AppResult<()> {
    let _lock = LOCK
        .lock()
        .map_err(|_| AppError::msg("Migration lock was poisoned"))?;
    recover(root, cache)?;
    let journal_path = root.join("_library_config").join(JOURNAL);
    let mut journal: Value = match read_optional(&journal_path)? {
        Some(bytes) => serde_json::from_slice(&bytes)?,
        None => json!({"version":0,"applied":[]}),
    };
    let current = version(&journal)?;
    if current > LATEST_VERSION {
        return Err(AppError::msg(
            "This library was migrated by a newer Toybox. Install that version or newer.",
        ));
    }
    if current == LATEST_VERSION {
        return Ok(());
    }
    for &(next, name) in STEPS.iter().filter(|(next, _)| *next > current) {
        let manifest_path = root.join(config::SEED_REL);
        let before = fsio::read_bytes(&manifest_path)?;
        let mut manifest: Value = serde_json::from_slice(&before)?;
        if !manifest["assets"].is_array() {
            return Err(AppError::msg(
                "Cannot migrate library: catalog assets must be an array",
            ));
        }
        let cached = read_optional(cache)?;
        let mut cached_doc: Option<Value> = cached
            .as_ref()
            .map(|bytes| serde_json::from_slice(bytes))
            .transpose()?;
        let root_matches = cached_doc
            .as_ref()
            .and_then(|doc| doc["libraryRoot"].as_str())
            .is_some_and(|value| {
                Path::new(value) == root
                    || Path::new(value)
                        .canonicalize()
                        .ok()
                        .zip(root.canonicalize().ok())
                        .is_some_and(|(a, b)| a == b)
            });
        if !root_matches {
            cached_doc = None;
        }
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
        let mut writes = vec![(Slot::Manifest, Some(before), encoded(&manifest)?)];
        if let Some(doc) = cached_doc {
            writes.push((Slot::Cache, cached, encoded(&doc)?));
        }
        writes.push((
            Slot::Journal,
            read_optional(&journal_path)?,
            encoded(&journal)?,
        ));
        commit(root, cache, next, writes)?;
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

fn commit(
    root: &Path,
    cache: &Path,
    version: u32,
    writes: Vec<(Slot, Option<Vec<u8>>, Vec<u8>)>,
) -> AppResult<()> {
    let writes: Vec<_> = writes
        .into_iter()
        .filter(|(_, before, after)| before.as_ref() != Some(after))
        .collect();
    let directory = format!(
        "{version:04}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AppError::msg("Invalid system time"))?
            .as_nanos()
    );
    let config = root.join("_library_config");
    let backup = config.join("migration-backups").join(&directory);
    let mut files = Vec::new();
    // All originals and replacements reach disk before any original is changed.
    for (slot, before, after) in &writes {
        if let Some(bytes) = before {
            fsio::write_bytes(&backup.join(format!("{}.before.json", slot.name())), bytes)?;
        }
        fsio::write_bytes(&backup.join(format!("{}.after.json", slot.name())), after)?;
        files.push(Snapshot {
            slot: *slot,
            before_exists: before.is_some(),
        });
    }
    let pending = Pending {
        directory,
        version,
        files,
    };
    fsio::write_bytes_atomic(&config.join(PENDING), &serde_json::to_vec_pretty(&pending)?)?;
    let result = (|| {
        for (slot, _, after) in &writes {
            fsio::write_bytes_atomic(&slot.path(root, cache), after)?;
        }
        fsio::remove_file_if_exists(&config.join(PENDING))
    })();
    if result.is_err() {
        recover(root, cache)?;
    }
    result
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
        let cache = root.join("appdata/catalog.json");
        fsio::write_bytes(&cache,&encoded(&json!({"libraryRoot":root,"assets":[{"id":"a","pack":"original_pack","user":{"favorite":true,"tags":["custom"],"collections":["Game"]}}]})).unwrap()).unwrap();
        (root, cache)
    }
    #[test]
    fn upgrades_once_preserves_metadata_and_keeps_backups() {
        let (root, cache) = fixture("once");
        run(&root, &cache).unwrap();
        let doc: Value = serde_json::from_slice(&fsio::read_bytes(&cache).unwrap()).unwrap();
        assert_eq!(
            doc["assets"][0]["user"]["provenance"]["source"],
            "original_pack"
        );
        assert_eq!(doc["assets"][0]["user"]["favorite"], true);
        let manifest = fsio::read_bytes(&root.join(config::SEED_REL)).unwrap();
        let journal = fsio::read_bytes(&root.join("_library_config").join(JOURNAL)).unwrap();
        run(&root, &cache).unwrap();
        assert_eq!(
            fsio::read_bytes(&root.join(config::SEED_REL)).unwrap(),
            manifest
        );
        assert_eq!(
            fsio::read_bytes(&root.join("_library_config").join(JOURNAL)).unwrap(),
            journal
        );
        assert_eq!(
            std::fs::read_dir(root.join("_library_config/migration-backups"))
                .unwrap()
                .count(),
            2
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
}
