//! Managed, immutable file sets grouped only by asset type. Pack/category are
//! metadata, never locations. Copy and verify before publishing either catalog;
//! a durable journal makes interrupted publication/cleanup resumable.
use crate::{
    domain::{
        catalog_migrate,
        catalog_model::Catalog,
        gltf_uri,
        seed::{self, SeedContent},
        tags,
    },
    error::{AppError, AppResult},
    infra::{
        fsio, retention,
        store::{self, CatalogWrite, Delta, Store},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Read,
    path::{Component, Path, PathBuf},
};

const PENDING: &str = "_library_config/storage-pending.json";
const LAYOUT: u64 = 1;
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn process_lock(root: &Path) -> AppResult<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("_library_config/storage.lock"))?;
    file.try_lock().map_err(|error|AppError::msg(format!("Another Toybox process is changing this library, or its storage lock is unavailable. Close the other instance and retry: {error}")))?;
    Ok(file)
}

#[derive(Clone, Serialize, Deserialize)]
struct CopyFile {
    source: String,
    target: String,
    digest: String,
    target_digest: String,
}
#[derive(Serialize, Deserialize)]
struct Transaction {
    manifest_before: Value,
    manifest_after: Value,
    cache_before: Option<Value>,
    cache_after: Option<Value>,
    #[serde(default)]
    migration_before: Option<Value>,
    #[serde(default)]
    migration_after: Option<Value>,
    cache_path: String,
    files: Vec<CopyFile>,
    legacy_dirs: Vec<String>,
    archive: String,
}

#[derive(Default)]
struct Plan {
    files: BTreeMap<String, CopyFile>,
    replacements: HashMap<String, String>,
    documents: BTreeMap<String, Vec<u8>>,
    hashes: HashMap<String, String>,
    contexts: HashMap<String, HashMap<String, String>>,
}

/// The managed folder under `library/` for an entry: one per native kind, and
/// one named for the type id of each plugin-defined type.
pub fn type_folder(entry: &Value) -> AppResult<String> {
    match entry["kind"].as_str().unwrap_or("model") {
        "model" => Ok("models".into()),
        "material" => Ok("materials".into()),
        "animation" => Ok("animations".into()),
        "audio" => Ok("audio".into()),
        "plugin" => entry["type"].as_str().filter(|id| seed::valid_type_id(id)).map(String::from)
            .ok_or_else(|| AppError::msg("Unknown asset storage type")),
        _ => Err(AppError::msg("Unknown asset storage type")),
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hash_file(path: &Path) -> AppResult<String> {
    let mut file = std::fs::File::open(path).map_err(|e| AppError::file("Read", path, e))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Check both lexical paths and every existing ancestor (including junctions).
fn under(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let path = Path::new(relative);
    if relative.contains(['\\', ':'])
        || !relative.starts_with("library/")
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(AppError::msg(format!(
            "Storage path escapes library: {relative}"
        )));
    }
    let path = root.join(path);
    let existing = path
        .ancestors()
        .find(|p| p.exists())
        .ok_or_else(|| AppError::msg("Missing library root"))?;
    if !existing.canonicalize()?.starts_with(root) {
        return Err(AppError::msg(format!(
            "Storage path leaves library: {relative}"
        )));
    }
    Ok(path)
}

fn basename(path: &str) -> AppResult<String> {
    Path::new(path)
        .file_name()
        .and_then(|p| p.to_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::msg("Missing asset file name"))
}

impl Plan {
    fn digest(&mut self, root: &Path, source: &str) -> AppResult<String> {
        if let Some(hash) = self.hashes.get(source) {
            return Ok(hash.clone());
        }
        let hash = hash_file(&under(root, source)?)?;
        self.hashes.insert(source.into(), hash.clone());
        Ok(hash)
    }
    fn copy(&mut self, root: &Path, source: &str, target: String) -> AppResult<String> {
        let digest = self.digest(root, source)?;
        if let Some(prior) = self.files.get(&target) {
            if prior.digest != digest {
                return Err(AppError::msg("Managed file collision"));
            }
        }
        self.files.insert(
            target.clone(),
            CopyFile {
                source: source.into(),
                target: target.clone(),
                target_digest: digest.clone(),
                digest,
            },
        );
        self.replacements
            .entry(source.into())
            .or_insert_with(|| target.clone());
        Ok(target)
    }
    fn entry(&mut self, root: &Path, entry: &mut Value) -> AppResult<()> {
        // Library tags are written the way they are read: normalised.
        tags::normalize_entry(entry);
        let parsed = seed::parse_entry(entry)?;
        // Variants own their file sets and keep their existing identity.
        if let Some(variants) = entry.get_mut("variants").and_then(Value::as_array_mut) {
            for variant in variants {
                self.entry(root, variant)?;
            }
        }
        if entry.get("storagePath").and_then(Value::as_str).is_some() {
            if entry["pack"]
                .as_str()
                .is_some_and(|p| p.starts_with("_import_"))
            {
                entry["pack"] = json!("imported");
            }
            seed::parse_entry(entry)?;
            return Ok(());
        }
        let kind = entry["kind"].as_str().unwrap_or("model").to_string();
        let type_root = format!("library/{}", type_folder(entry)?);
        // The immutable set key includes identity, declaration and file contents.
        // Updating an item publishes new paths instead of modifying live files.
        let mut key = Sha256::new();
        key.update(parsed.id.as_bytes());
        // Library tags are descriptive metadata that change after the fact, so they
        // are not part of the set's identity: editing them never moves a file.
        // (Provenance is part of it: it is written into the entry when the item is
        // first stored, and an item that is stored already is never keyed again.)
        key.update(serde_json::to_vec(&tags::without_tags(entry))?);
        let mut doc = None;
        let mut dependencies = Vec::new();
        // Source files the importing plugin keeps with the item (a material's
        // archive): part of its identity, stored in its folder.
        let sources: Vec<String> = parsed.origin.as_ref().map(|origin| origin.files.clone()).unwrap_or_default();
        for file in &sources {
            key.update(self.digest(root, file)?.as_bytes());
        }
        match &parsed.content {
            SeedContent::Model { gltf_rel, .. } | SeedContent::AnimationSource { gltf_rel, .. } => {
                let mut scene: Value =
                    serde_json::from_slice(&fsio::read_bytes(&under(root, gltf_rel)?)?)?;
                if scene["buffers"].as_array().is_some_and(|b| b.len() > 1) {
                    return Err(AppError::msg(format!(
                        "Cannot migrate multi-buffer scene: {gltf_rel}"
                    )));
                }
                let parent = Path::new(gltf_rel)
                    .parent()
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                for field in ["buffers", "images"] {
                    if let Some(resources) = scene[field].as_array() {
                        for (index, resource) in resources.iter().enumerate() {
                            let Some(uri) = resource["uri"].as_str() else {
                                continue;
                            };
                            // Embedded (`data:`) and remote references are not
                            // library files: they stay in the scene as written.
                            let Some(source) = gltf_uri::resolve_library_file(&parent, uri)? else {
                                continue;
                            };
                            let hash = self.digest(root, &source)?;
                            key.update(hash.as_bytes());
                            dependencies.push((field, index, source, hash));
                        }
                    }
                }
                // Preserve path-derived animation IDs when a pre-extraction
                // model is first opened by the upgraded application.
                if scene["extras"]["toyboxSourcePath"].is_null() {
                    if scene["extras"].is_null() {
                        scene["extras"] = json!({});
                    }
                    if scene["extras"].is_object() {
                        scene["extras"]["toyboxSourcePath"] = json!(gltf_rel);
                    } else {
                        if scene["extensions"].is_null() {
                            scene["extensions"] = json!({});
                        }
                        scene["extensions"]["TOYBOX_library"] = json!({"sourcePath":gltf_rel});
                    }
                }
                key.update(self.digest(root, gltf_rel)?.as_bytes());
                doc = Some(scene);
            }
            SeedContent::Material { material } => {
                for file in material.textures() {
                    key.update(self.digest(root, &file)?.as_bytes());
                    dependencies.push(("maps", 0, file.clone(), String::new()));
                }
            }
            SeedContent::Audio { file } => {
                key.update(self.digest(root, file)?.as_bytes());
                dependencies.push(("audio", 0, file.clone(), String::new()));
            }
            SeedContent::Plugin { files, .. } => {
                for file in files {
                    key.update(self.digest(root, file)?.as_bytes());
                    dependencies.push(("files", 0, file.clone(), String::new()));
                }
            }
        }
        let folder = format!("{type_root}/{:x}", key.finalize());
        let mut local = HashMap::new();
        let mut context = HashMap::new();
        if let Some(mut scene) = doc {
            let target = format!("{folder}/{}", basename(&parsed.file_name)?);
            let bin = Path::new(&target)
                .with_extension("bin")
                .to_string_lossy()
                .replace('\\', "/");
            for (field, index, source, hash) in dependencies {
                let destination = if field == "buffers" {
                    bin.clone()
                } else {
                    let extension = Path::new(&source)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("bin")
                        .to_lowercase();
                    format!("{type_root}/_resources/{hash}.{extension}")
                };
                self.copy(root, &source, destination.clone())?;
                context.insert(source.clone(), destination.clone());
                scene[field][index]["uri"] = json!(if field == "buffers" {
                    basename(&bin)?
                } else {
                    format!("../_resources/{}", basename(&destination)?)
                });
            }
            // Source files the importer kept (a model's original file) sit
            // beside the glTF and bin; their names were checked against both.
            for source in &sources {
                let name = basename(source)?;
                let kept = self.copy(root, source, format!("{folder}/{name}"))?;
                context.insert(source.clone(), kept);
            }
            context.insert(parsed.rel_path.clone(), target.clone());
            if let SeedContent::Model { bin_rel, .. }
            | SeedContent::AnimationSource { bin_rel, .. } = &parsed.content
            {
                context.insert(bin_rel.clone(), bin);
            }
            self.replacements.insert(parsed.rel_path, target.clone());
            self.documents
                .insert(target.clone(), serde_json::to_vec_pretty(&scene)?);
            entry["storagePath"] = json!(target);
        } else if kind == "plugin" {
            // File names are unique within a set (checked when parsing), so
            // each keeps its own name inside the set's folder.
            for (_, _, source, _) in dependencies {
                let target = self.copy(root, &source, format!("{folder}/{}", basename(&source)?))?;
                context.insert(source, target);
            }
            context.insert(parsed.rel_path.clone(), folder.clone());
            self.replacements.insert(parsed.rel_path, folder.clone());
            entry["storagePath"] = json!(folder);
        } else if kind == "material" {
            for (_, _, source, _) in dependencies {
                let plain = basename(&source)?;
                let hash = self.digest(root, &source)?;
                let collision = local
                    .values()
                    .any(|(name, _): &(String, String)| name == &plain);
                let name = if collision {
                    format!("{}-{plain}", &hash[..16])
                } else {
                    plain
                };
                let target = self.copy(root, &source, format!("{folder}/{name}"))?;
                context.insert(source.clone(), target.clone());
                local.insert(source, (name, target));
            }
            if let Some(maps) = entry["maps"].as_array_mut() {
                let SeedContent::Material { material } = parsed.content else {
                    unreachable!()
                };
                for (map, resolved) in maps.iter_mut().zip(material.maps) {
                    map["file"] = json!(local[&resolved.file].0);
                }
            }
            for source in &sources {
                let name = basename(source)?;
                if local.values().any(|(taken, _)| taken.eq_ignore_ascii_case(&name)) {
                    return Err(AppError::msg(format!("Source file {name} shares a name with a map of its material")));
                }
                let target = self.copy(root, source, format!("{folder}/{name}"))?;
                context.insert(source.clone(), target);
            }
            context.insert(parsed.rel_path.clone(), folder.clone());
            self.replacements.insert(parsed.rel_path, folder.clone());
            entry["storagePath"] = json!(folder);
        } else {
            let source = &dependencies[0].2;
            let target = self.copy(root, source, format!("{folder}/{}", basename(source)?))?;
            context.insert(source.clone(), target.clone());
            entry["storagePath"] = json!(target);
        }
        self.contexts.insert(parsed.id, context);
        restore_transferred(entry);
        // Compatibility field for older plugins; this is no longer a folder
        // or a source identity. Explicit provenance carries the source title.
        if entry["pack"]
            .as_str()
            .is_some_and(|p| p.starts_with("_import_"))
        {
            entry["pack"] = json!("imported");
        }
        Ok(())
    }
}

/// A transferred entry is staged under a private pack and category (so equal
/// file names of different items cannot meet) and carries the item's own in
/// `transferOf`. Put them back, so a transfer keeps pack and category the way
/// an importer's output never could.
fn restore_transferred(entry: &mut Value) {
    let Some(own) = entry.as_object_mut().and_then(|object| object.remove("transferOf")) else { return };
    for key in ["pack", "category"] {
        if let Some(value) = own.get(key).filter(|value| value.is_string()) {
            entry[key] = value.clone();
        }
    }
}

fn update_cache(
    value: &mut Value,
    replacements: &HashMap<String, String>,
    contexts: &HashMap<String, HashMap<String, String>>,
) {
    // Only media references change; provenance, names, IDs, rigs, collections,
    // favorites and preview selections are not inferred or rewritten.
    fn replace(value: &mut Value, map: &HashMap<String, String>) {
        if let Some(next) = value.as_str().and_then(|s| map.get(s)) {
            *value = json!(next);
        }
    }
    if let Some(entries) = value["assets"].as_array_mut() {
        for entry in entries {
            let references = entry["id"]
                .as_str()
                .and_then(|id| contexts.get(id))
                .unwrap_or(replacements);
            replace(&mut entry["relPath"], references);
            if let Some(fileset) = entry.get_mut("fileset") {
                replace(&mut fileset["gltf"], references);
                replace(&mut fileset["bin"], references);
                if let Some(textures) = fileset["textures"].as_array_mut() {
                    for file in textures {
                        replace(file, references);
                    }
                }
            }
            if let Some(audio) = entry.get_mut("audio") {
                replace(&mut audio["file"], references);
            }
            if let Some(files) = entry.get_mut("files").and_then(Value::as_array_mut) {
                for file in files {
                    replace(file, references);
                }
            }
            if let Some(files) = entry.get_mut("origin").and_then(|origin| origin.get_mut("files")).and_then(Value::as_array_mut) {
                for file in files {
                    replace(file, references);
                }
            }
            if let Some(maps) = entry
                .get_mut("material")
                .and_then(|m| m["maps"].as_array_mut())
            {
                for map in maps {
                    replace(&mut map["file"], references);
                }
            }
            if let Some(variants) = entry.get_mut("variants") {
                let mut wrapper = json!({"assets":variants});
                update_cache(&mut wrapper, replacements, contexts);
                *variants = wrapper["assets"].take();
            }
        }
    }
}

/// Upgrade existing media without reindexing, extracting, or enriching assets.
pub fn migrate(root: &Path, cache: &Path) -> AppResult<()> {
    let _guard = LOCK
        .lock()
        .map_err(|_| AppError::msg("Storage migration lock was poisoned"))?;
    let root = root.canonicalize()?;
    let _process_guard = process_lock(&root)?;
    recover_inner(&root, cache)?;
    let before = store::read_manifest(&root)?;
    let after = before.clone();
    publish_inner(&root, cache, before, after)
}

/// Run `work` with this library exclusively locked, as a publication does (the
/// in-process lock and the cross-process file lock), after finishing any
/// interrupted publication. For a writer of the manifest that does not publish
/// files itself (a metadata edit): it must not call `publish` from inside.
pub fn exclusive<T>(root: &Path, cache: &Path, work: impl FnOnce(&Path) -> AppResult<T>) -> AppResult<T> {
    let _guard = LOCK
        .lock()
        .map_err(|_| AppError::msg("Storage migration lock was poisoned"))?;
    let root = root.canonicalize()?;
    let _process_guard = process_lock(&root)?;
    recover_inner(&root, cache)?;
    work(&root)
}

/// The host normalizes every importer through this same storage contract.
pub fn publish(root: &Path, cache: &Path, before: Value, after: Value) -> AppResult<()> {
    let _guard = LOCK
        .lock()
        .map_err(|_| AppError::msg("Storage migration lock was poisoned"))?;
    let _process_guard = process_lock(root)?;
    publish_inner(root, cache, before, after)
}
fn publish_inner(root: &Path, cache: &Path, before: Value, mut after: Value) -> AppResult<()> {
    let root = root.canonicalize()?;
    recover_inner(&root, cache)?;
    let staging_dirs: Vec<_> = after["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["pack"].as_str())
        .filter(|pack| pack.starts_with("_import_") && !pack.contains(['/', '\\', ':']))
        .map(|pack| format!("library/{pack}"))
        .collect();
    let mut plan = Plan::default();
    for entry in after["assets"]
        .as_array_mut()
        .ok_or_else(|| AppError::msg("Missing source assets"))?
    {
        plan.entry(&root, entry)?;
    }
    after["storageVersion"] = json!(LAYOUT);
    if before == after {
        return Ok(());
    }
    // The catalog of this library, as the document the journal records.
    let old_cache = Store::local(&root, cache).load_catalog()?.map(|catalog| serde_json::to_value(&catalog)).transpose()?;
    let mut new_cache = old_cache.clone();
    if let Some(doc) = &mut new_cache {
        update_cache(doc, &plan.replacements, &plan.contexts);
    }
    // Copies are immutable and unreferenced until the manifest is published.
    // Failed preflight/copy leaves both the current catalog and originals intact.
    for file in plan.files.values() {
        let target = under(&root, &file.target)?;
        if target.exists() {
            if hash_file(&target)? != file.digest {
                return Err(AppError::msg(
                    "Managed storage collision; destination differs",
                ));
            }
            continue;
        }
        let source = under(&root, &file.source)?;
        if hash_file(&source)? != file.digest {
            return Err(AppError::msg(
                "Asset changed during storage migration; retry",
            ));
        }
        let pending = target.with_extension("copying");
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::fs::copy(&source, &pending)?;
        if hash_file(&pending)? != file.digest {
            return Err(AppError::msg("Storage copy verification failed"));
        }
        std::fs::rename(pending, target)?;
    }
    for (relative, bytes) in &plan.documents {
        let target = under(&root, relative)?;
        if target.exists() {
            if fsio::read_bytes(&target)? != *bytes {
                return Err(AppError::msg("Managed scene collision"));
            }
        } else {
            fsio::write_bytes_atomic(&target, bytes)?;
        }
    }
    let (current, revision) = store::read_manifest_with_revision(&root)?;
    if current != before {
        return Err(AppError::msg(
            "Library changed during storage migration; retry",
        ));
    }
    // Back up metadata before publication. Media remains at its original path
    // until the durable transaction has published both documents.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let backup = root.join(format!("_library_config/migration-backups/storage-{stamp}"));
    fsio::write_bytes(&backup.join("manifest.before.json"), &serde_json::to_vec_pretty(&current)?)?;
    if let Some(old) = &old_cache {
        fsio::write_bytes(&backup.join("cache.before.json"), &serde_json::to_vec_pretty(old)?)?;
    }
    for (source, _) in plan
        .replacements
        .iter()
        .filter(|(_, target)| plan.documents.contains_key(*target))
    {
        fsio::write_bytes(
            &backup
                .join("scenes")
                .join(format!("{}.gltf", hash_bytes(source.as_bytes()))),
            &fsio::read_bytes(&under(&root, source)?)?,
        )?;
    }
    let mut files: Vec<_> = plan.files.into_values().collect();
    // Rewritten scene files also retire their original physical documents.
    for (source, target) in plan.replacements {
        if plan.documents.contains_key(&target) && !files.iter().any(|f| f.source == source) {
            files.push(CopyFile {
                digest: plan.hashes[&source].clone(),
                target_digest: hash_bytes(&plan.documents[&target]),
                source,
                target,
            });
        }
    }
    let mut legacy_dirs = HashSet::new();
    legacy_dirs.extend(staging_dirs);
    for entry in before["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(after["assets"].as_array().into_iter().flatten())
    {
        if let Some(pack) = entry["pack"].as_str() {
            if (before["storageVersion"] != LAYOUT || pack.starts_with("_import_"))
                && !pack.is_empty()
                && !pack.contains(['/', '\\', ':'])
                && !matches!(
                    pack,
                    "." | ".." | "models" | "materials" | "animations" | "audio"
                )
            {
                legacy_dirs.insert(format!("library/{pack}"));
            }
        }
    }
    // Make the minimum readable migration version durable before retiring any
    // legacy media. A crash cannot expose moved media to an older reader.
    let migration_path = root.join("_library_config/migrations.json");
    let migration_before = if migration_path.exists() {
        Some(serde_json::from_slice::<Value>(&fsio::read_bytes(
            &migration_path,
        )?)?)
    } else {
        None
    };
    let migration_after = if migration_before.as_ref().is_some_and(|j| j["version"] == 2) {
        let mut journal = migration_before.clone().unwrap();
        journal["version"] = json!(3);
        journal["applied"].as_array_mut().ok_or_else(||AppError::msg("Invalid migration history"))?.push(json!({"version":3,"name":"typed-media-storage","appVersion":env!("CARGO_PKG_VERSION")}));
        Some(journal)
    } else {
        None
    };
    if let Some(journal) = &migration_before {
        fsio::write_bytes(
            &backup.join("journal.before.json"),
            &serde_json::to_vec_pretty(journal)?,
        )?;
    }
    let transaction = Transaction {
        manifest_before: before,
        manifest_after: after,
        cache_before: old_cache,
        cache_after: new_cache,
        migration_before,
        migration_after,
        cache_path: cache.to_string_lossy().into(),
        files,
        legacy_dirs: legacy_dirs.into_iter().collect(),
        archive: format!("_library_config/migration-backups/storage-{stamp}/unindexed"),
    };
    // The snapshot's own copy of the transaction is a record of what moved. The
    // four whole documents inside it are already saved as `manifest.before.json`
    // and `cache.before.json` (and the "after" ones are the live files once
    // published), so they are left out here; recovery reads `storage-pending.json`,
    // which stays complete.
    let mut record = serde_json::to_value(&transaction)?;
    for field in ["manifest_before", "manifest_after", "cache_before", "cache_after"] {
        if !record[field].is_null() {
            record[field] = json!("omitted: see manifest.before.json / cache.before.json");
        }
    }
    fsio::write_bytes(&backup.join("transaction.json"), &serde_json::to_vec_pretty(&record)?)?;
    fsio::write_bytes_atomic(
        &root.join(PENDING),
        &serde_json::to_vec_pretty(&transaction)?,
    )?;
    finish(&root, cache, transaction, Some(revision))?;
    prune_snapshots(&root);
    Ok(())
}

/// Keep the newest few `storage-<nanos>` publication snapshots (see
/// `retention`). Needs the storage locks to be held by the caller. Never fails.
/// Skips a snapshot holding archived source folders (`unindexed`, the user's
/// own leftover files) and the one an unfinished journal still points at.
fn prune_snapshots(root: &Path) {
    let backups = root.join("_library_config/migration-backups");
    let pending = fsio::read_bytes(&root.join(PENDING))
        .ok()
        .or_else(|| fsio::read_bytes(&root.join(PENDING_ITEMS)).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|journal| journal["archive"].as_str().map(String::from))
        .and_then(|archive| Path::new(&archive).parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()));
    retention::prune_newest(
        &backups,
        retention::STORAGE_SNAPSHOT_PREFIX,
        "",
        retention::KEEP_STORAGE_SNAPSHOTS,
        &|name| pending.as_deref() == Some(name) || backups.join(name).join("unindexed").exists(),
    );
}

/// Open-time tidy-up of a library's storage: prune old publication snapshots
/// and remove empty set folders. Runs only when no publication is in progress
/// (in this process or another), and returns whether it ran.
pub fn tidy(root: &Path) -> bool {
    let Ok(_guard) = LOCK.try_lock() else { return false };
    let Ok(root) = root.canonicalize() else { return true };
    let Ok(_process_guard) = process_lock(&root) else { return false };
    prune_snapshots(&root);
    retention::sweep_empty_set_folders(&root);
    true
}

pub fn recover(root: &Path, cache: &Path) -> AppResult<()> {
    let _guard = LOCK
        .lock()
        .map_err(|_| AppError::msg("Storage migration lock was poisoned"))?;
    // `under` compares canonical paths with `root`, so it must be canonical
    // too (a symlinked data folder, or a `\\?\` prefix on Windows).
    let root = root.canonicalize()?;
    let _process_guard = process_lock(&root)?;
    recover_inner(&root, cache)
}
fn recover_inner(root: &Path, cache: &Path) -> AppResult<()> {
    let items_path = root.join(PENDING_ITEMS);
    if items_path.exists() {
        let transaction: ItemsTransaction = serde_json::from_slice(&fsio::read_bytes(&items_path)?)?;
        finish_items(root, cache, transaction, None)?;
    }
    let path = root.join(PENDING);
    if !path.exists() {
        return Ok(());
    }
    let mut transaction: Transaction = serde_json::from_slice(&fsio::read_bytes(&path)?)?;
    let recorded = Path::new(&transaction.cache_path);
    if recorded != cache {
        // A library folder copied to another machine (a Windows library put
        // on a Linux server, or back) carries the journal with the cache path
        // of the machine that began the change. That path means nothing here,
        // and the cache is not part of the library: finish the library's own
        // files and leave the cache to be rebuilt. A different cache path on
        // this same kind of host is still refused.
        if recorded.is_absolute() {
            return Err(AppError::msg(
                "Storage recovery requires its original catalog cache",
            ));
        }
        transaction.cache_before = None;
        transaction.cache_after = None;
        transaction.cache_path = cache.to_string_lossy().into();
    }
    finish(root, cache, transaction, None)
}

/// Complete a recorded publication: both documents in one database commit (the
/// manifest and the catalog rows), then the migration journal, the files, the
/// archive. Every step is repeatable. `expect` is the manifest revision the
/// publication was planned against (`None` when finishing an interrupted one).
fn finish(root: &Path, cache: &Path, transaction: Transaction, expect: Option<i64>) -> AppResult<()> {
    if let Some(after) = &transaction.migration_after {
        let current: Value = serde_json::from_slice(&fsio::read_bytes(
            &root.join("_library_config/migrations.json"),
        )?)?;
        if Some(&current) != transaction.migration_before.as_ref() && &current != after {
            return Err(AppError::msg(
                "Storage recovery stopped: migration journal was edited externally",
            ));
        }
    }
    let store = Store::local(root, cache);
    let current = store::read_manifest(root)?;
    if current != transaction.manifest_before && current != transaction.manifest_after {
        return Err(AppError::msg(
            "Storage recovery stopped: manifest was edited externally",
        ));
    }
    let catalog_after: Option<Catalog> = transaction.cache_after.clone().map(|doc| serde_json::from_value(catalog_migrate::upgrade(doc))).transpose()?;
    if let Some(after) = &transaction.cache_after {
        let current = store.load_catalog()?.map(|catalog| serde_json::to_value(&catalog)).transpose()?;
        if !store::same_catalog(current.as_ref(), transaction.cache_before.as_ref()) && !store::same_catalog(current.as_ref(), Some(after)) {
            return Err(AppError::msg(
                "Storage recovery stopped: catalog was edited externally",
            ));
        }
    }
    for file in &transaction.files {
        let target = under(root, &file.target)?;
        if hash_file(&target)? != file.target_digest {
            return Err(AppError::msg(
                "Storage recovery found a missing or modified destination file",
            ));
        }
    }
    // One transaction on the library database (plus the personal database's,
    // opened before it): a crash leaves the old documents or the new ones.
    store.commit(CatalogWrite { manifest: Some(&transaction.manifest_after), catalog: catalog_after.as_ref(), expect_manifest_revision: expect })?;
    if let Some(after) = &transaction.migration_after {
        fsio::write_bytes_atomic(
            &root.join("_library_config/migrations.json"),
            &serde_json::to_vec_pretty(after)?,
        )?;
    }
    retire(root, &transaction.files, &transaction.legacy_dirs, &transaction.archive)?;
    fsio::remove_file_if_exists(&root.join(PENDING))
}

// ---- publishing some items --------------------------------------------------

const PENDING_ITEMS: &str = "_library_config/storage-pending-items.json";

/// The journal of a publication of some items: only what the commit touches.
#[derive(Serialize, Deserialize)]
struct ItemsTransaction {
    delta: Delta,
    /// The entries of the touched items before the change, by id (absent: new).
    before: BTreeMap<String, Value>,
    files: Vec<CopyFile>,
    legacy_dirs: Vec<String>,
    archive: String,
}

/// The storage layout of some entries about to be added to a library: which
/// managed files they need, planned without touching anything. Create it with
/// [`ItemPlan::new`], place the files with [`ItemPlan::place`], derive whatever the
/// stored entries need, then publish with [`ItemPlan::commit`]. All of it runs
/// inside [`exclusive`].
pub struct ItemPlan {
    root: PathBuf,
    plan: Plan,
    entries: Vec<Value>,
    staging_dirs: Vec<String>,
    /// Files and documents this plan created (removed again by `discard`).
    created: Vec<PathBuf>,
}

impl ItemPlan {
    /// Plan `entries`, in the layout every importer's output gets: tags
    /// normalised, a storage folder per item named by its content, shared images
    /// in `_resources`. `root` is the canonical root `exclusive` hands out.
    pub fn new(root: &Path, mut entries: Vec<Value>) -> AppResult<ItemPlan> {
        let staging_dirs: Vec<String> = entries
            .iter()
            .filter_map(|entry| entry["pack"].as_str())
            .filter(|pack| pack.starts_with("_import_") && !pack.contains(['/', '\\', ':']))
            .map(|pack| format!("library/{pack}"))
            .collect();
        let mut plan = Plan::default();
        for entry in entries.iter_mut() {
            plan.entry(root, entry)?;
        }
        Ok(ItemPlan { root: root.to_path_buf(), plan, entries, staging_dirs, created: Vec::new() })
    }

    /// The entries as they will be stored (with their `storagePath`).
    pub fn entries(&self) -> &[Value] {
        &self.entries
    }

    /// Copy the planned files and write the rewritten scenes. Nothing is
    /// referenced yet; what this creates is removed by [`ItemPlan::discard`].
    pub fn place(&mut self) -> AppResult<()> {
        let result = self.place_inner();
        if result.is_err() {
            self.discard();
        }
        result
    }

    fn place_inner(&mut self) -> AppResult<()> {
        let root = self.root.clone();
        for file in self.plan.files.values() {
            let target = under(&root, &file.target)?;
            if target.exists() {
                if hash_file(&target)? != file.digest {
                    return Err(AppError::msg("Managed storage collision; destination differs"));
                }
                continue;
            }
            let source = under(&root, &file.source)?;
            if hash_file(&source)? != file.digest {
                return Err(AppError::msg("Asset changed during storage migration; retry"));
            }
            let pending = target.with_extension("copying");
            std::fs::create_dir_all(target.parent().unwrap())?;
            std::fs::copy(&source, &pending)?;
            if hash_file(&pending)? != file.digest {
                let _ = std::fs::remove_file(&pending);
                return Err(AppError::msg("Storage copy verification failed"));
            }
            std::fs::rename(pending, &target)?;
            self.created.push(target);
        }
        for (relative, bytes) in &self.plan.documents {
            let target = under(&root, relative)?;
            if target.exists() {
                if fsio::read_bytes(&target)? != *bytes {
                    return Err(AppError::msg("Managed scene collision"));
                }
            } else {
                fsio::write_bytes_atomic(&target, bytes)?;
                self.created.push(target);
            }
        }
        Ok(())
    }

    /// Remove the files [`ItemPlan::place`] created, and folders left empty.
    pub fn discard(&mut self) {
        for path in std::mem::take(&mut self.created) {
            let _ = std::fs::remove_file(&path);
            let mut parent = path.parent();
            while let Some(directory) = parent {
                if directory == self.root.join("library") || std::fs::remove_dir(directory).is_err() {
                    break;
                }
                parent = directory.parent();
            }
        }
    }

    /// Publish: record the change durably (a journal that holds only this
    /// commit's items), put it into the databases in one transaction, then retire
    /// the staged originals. `before` holds the entries of the touched items as
    /// they were. A failure before the journal is written removes the placed files.
    pub fn commit(mut self, cache: &Path, before: BTreeMap<String, Value>, mut delta: Delta) -> AppResult<()> {
        let result = self.commit_inner(cache, before, &mut delta);
        if result.is_err() && !self.root.join(PENDING_ITEMS).exists() {
            self.discard();
        }
        result
    }

    fn commit_inner(&mut self, cache: &Path, before: BTreeMap<String, Value>, delta: &mut Delta) -> AppResult<()> {
        let root = self.root.clone();
        // The library must still be as it was planned against; checked before anything is journaled.
        if let Some(expected) = delta.expect_manifest_revision {
            if store::manifest_revision(&root)? != expected {
                return Err(AppError::msg("Library changed during storage migration; retry"));
            }
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let backup = root.join(format!("_library_config/migration-backups/storage-{stamp}"));
        fsio::write_bytes(&backup.join("items.before.json"), &serde_json::to_vec_pretty(&before)?)?;
        for (source, _) in self.plan.replacements.iter().filter(|(_, target)| self.plan.documents.contains_key(*target)) {
            fsio::write_bytes(&backup.join("scenes").join(format!("{}.gltf", hash_bytes(source.as_bytes()))), &fsio::read_bytes(&under(&root, source)?)?)?;
        }
        let plan = std::mem::take(&mut self.plan);
        let mut files: Vec<_> = plan.files.into_values().collect();
        // Rewritten scene files also retire their original physical documents.
        for (source, target) in plan.replacements {
            if plan.documents.contains_key(&target) && !files.iter().any(|f| f.source == source) {
                files.push(CopyFile { digest: plan.hashes[&source].clone(), target_digest: hash_bytes(&plan.documents[&target]), source, target });
            }
        }
        delta.storage_version = Some(LAYOUT);
        let transaction = ItemsTransaction {
            delta: delta.clone(),
            before,
            files,
            legacy_dirs: std::mem::take(&mut self.staging_dirs),
            archive: format!("_library_config/migration-backups/storage-{stamp}/unindexed"),
        };
        fsio::write_bytes(&backup.join("transaction.json"), &serde_json::to_vec_pretty(&transaction)?)?;
        fsio::write_bytes_atomic(&root.join(PENDING_ITEMS), &serde_json::to_vec_pretty(&transaction)?)?;
        finish_items(&root, cache, transaction, delta.expect_manifest_revision)?;
        prune_snapshots(&root);
        Ok(())
    }
}

/// Complete a recorded publication of items: the rows in one transaction, then
/// the originals, the archive. Repeatable at every step. `expect` is the
/// manifest revision the publication was planned against (`None` when finishing
/// an interrupted one).
fn finish_items(root: &Path, cache: &Path, transaction: ItemsTransaction, expect: Option<i64>) -> AppResult<()> {
    let store = Store::local(root, cache);
    let mut ids: Vec<String> = transaction.delta.puts.iter().filter(|put| put.entry.is_some()).map(|put| put.id.clone()).collect();
    ids.extend(transaction.delta.remove_items.iter().cloned());
    let current: BTreeMap<String, Value> = store.load_entries(&ids)?.into_iter().filter_map(|entry| Some((entry.get("id")?.as_str()?.to_string(), entry))).collect();
    for put in transaction.delta.puts.iter().filter(|put| put.entry.is_some()) {
        let now = current.get(&put.id);
        if now != transaction.before.get(&put.id) && now != put.entry.as_ref() {
            return Err(AppError::msg("Storage recovery stopped: manifest was edited externally"));
        }
    }
    for id in &transaction.delta.remove_items {
        if current.get(id).is_some_and(|now| Some(now) != transaction.before.get(id)) {
            return Err(AppError::msg("Storage recovery stopped: manifest was edited externally"));
        }
    }
    for file in &transaction.files {
        let target = under(root, &file.target)?;
        if hash_file(&target)? != file.target_digest {
            return Err(AppError::msg("Storage recovery found a missing or modified destination file"));
        }
    }
    let mut delta = transaction.delta.clone();
    delta.expect_manifest_revision = expect;
    // A library whose catalog is not current takes the items into the manifest and
    // leaves the derived rows to the rescan that its next write will start.
    delta.skip_catalog = !store.catalog_ready();
    let started = std::time::Instant::now();
    store.apply(&delta)?;
    let applied = started.elapsed();
    retire(root, &transaction.files, &transaction.legacy_dirs, &transaction.archive)?;
    if std::env::var_os("TOYBOX_TRACE").is_some() {
        eprintln!("publish rows: {:.1} ms, retire originals: {:.1} ms", applied.as_secs_f64() * 1000.0, (started.elapsed() - applied).as_secs_f64() * 1000.0);
    }
    fsio::remove_file_if_exists(&root.join(PENDING_ITEMS))
}

/// After the documents are published: remove the originals of the files that were
/// copied (when they are still what was copied) and archive the staging folders.
/// Every step is repeatable.
fn retire(root: &Path, files: &[CopyFile], legacy_dirs: &[String], archive: &str) -> AppResult<()> {
    // Only retire explicitly migrated files, never recursively remove a pack.
    // Extra source files and unknown metadata are left untouched.
    let destinations: HashSet<_> = files.iter().map(|file| file.target.as_str()).collect();
    let mut removed = HashSet::new();
    for file in files {
        if destinations.contains(file.source.as_str()) || !removed.insert(&file.source) {
            continue;
        }
        let source = under(root, &file.source)?;
        if !source.exists() {
            continue;
        }
        if hash_file(&source)? != file.digest {
            return Err(AppError::msg(
                "An original file changed before cleanup; it has been kept",
            ));
        }
        fsio::remove_file_if_exists(&source)?;
        let mut parent = source.parent();
        while let Some(directory) = parent {
            if directory == root.join("library") {
                break;
            }
            if std::fs::remove_dir(directory).is_err() {
                break;
            }
            parent = directory.parent();
        }
    }
    // Preserve unindexed source extras and old pack.json files as migration
    // backups instead of retaining pack trees in the managed media directory.
    if !archive.starts_with("_library_config/migration-backups/storage-")
        || Path::new(archive)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(AppError::msg("Invalid storage backup location"));
    }
    let archive = root.join(archive);
    for relative in legacy_dirs {
        let source = under(root, relative)?;
        if !source.exists() {
            continue;
        }
        let destination = archive.join(basename(relative)?);
        std::fs::create_dir_all(&archive)?;
        if !archive.canonicalize()?.starts_with(root) || destination.exists() {
            return Err(AppError::msg(
                "Storage backup destination is occupied or outside library",
            ));
        }
        let mut moved = false;
        for attempt in 0..6 {
            match std::fs::rename(&source, &destination) {
                Ok(()) => {
                    moved = true;
                    break;
                }
                Err(error)
                    if cfg!(windows)
                        && attempt < 5
                        && matches!(error.raw_os_error(), Some(5 | 32 | 33)) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(100 << attempt))
                }
                Err(error) => {
                    return Err(AppError::msg(format!(
                        "Archive '{}' to '{}': {error}",
                        source.display(),
                        destination.display()
                    )))
                }
            }
        }
        if !moved {
            return Err(AppError::msg("Could not archive the old source folder"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config;
    use super::*;
    fn fixture(name: &str) -> (PathBuf, PathBuf) {
        // Unique per test even when two start in the same clock tick.
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let stamp = format!("{}-{}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        let root = std::env::temp_dir().join(format!("toybox-storage-{name}-{stamp}"));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let cache = root.join("appdata/catalog.json");
        (root, cache)
    }
    /// Write a file of the fixture library. The two documents that used to be JSON
    /// files (the manifest and the cache) go through the store, as the same JSON.
    fn write(root: &Path, path: &str, bytes: &[u8]) {
        if path == config::SEED_REL {
            store::testing::put_manifest(root, &serde_json::from_slice(bytes).unwrap());
        } else if path == "appdata/catalog.json" {
            store::testing::put_catalog_doc(root, &root.join(path), serde_json::from_slice(bytes).unwrap());
        } else {
            fsio::write_bytes(&root.join(path), bytes).unwrap();
        }
    }
    fn read(path: &Path) -> Value {
        if let Some(root) = path.parent().and_then(Path::parent).filter(|_| path.ends_with(config::SEED_REL)) {
            return store::testing::get_manifest(root);
        }
        if let Some(root) = path.parent().and_then(Path::parent).filter(|_| path.ends_with("appdata/catalog.json")) {
            return store::testing::get_catalog_doc(root, path);
        }
        serde_json::from_slice(&fsio::read_bytes(path).unwrap()).unwrap()
    }
    fn scene(texture: &str) -> Value {
        json!({"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"Root"}],"buffers":[{"uri":"Hero.bin","byteLength":4}],"images":[{"uri":texture}]})
    }
    #[test]
    fn all_types_variants_shared_resources_and_user_metadata_survive() {
        let (root, cache) = fixture("types");
        for pack in ["one", "two"] {
            write(
                &root,
                &format!("library/{pack}/characters/Hero.gltf"),
                &serde_json::to_vec(&scene("../textures/Atlas.png")).unwrap(),
            );
            write(
                &root,
                &format!("library/{pack}/characters/Hero.bin"),
                b"mesh",
            );
            write(
                &root,
                &format!("library/{pack}/textures/Atlas.png"),
                if pack == "one" {
                    b"first atlas"
                } else {
                    b"second atlas"
                },
            );
        }
        write(&root, "library/one/materials/Stone/color.png", b"stone");
        write(&root, "library/one/materials/Stone_1K/color.png", b"small");
        write(&root, "library/one/sounds/click.wav", b"sound");
        write(&root,"library/one/animations/Wave.gltf",br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"Wave.bin","byteLength":4}],"extras":{"toyboxAnimation":{"id":"clip","clip":{"sourceId":"one"}}}}"#);
        write(&root, "library/one/animations/Wave.bin", b"anim");
        write(
            &root,
            "library/one/pack.json",
            br#"{"name":"Original source"}"#,
        );
        write(&root, "library/one/raw/source.fbx", b"unindexed original");
        let assets = json!([
            {"id":"one","kind":"model","pack":"one","category":"characters","file":"Hero.gltf","provenance":{"source":"Original source","license":"CC0"}},
            {"id":"two","kind":"model","pack":"two","category":"characters","file":"Hero.gltf"},
            {"id":"stone","kind":"material","pack":"one","category":"stone","name":"Stone","maps":[{"role":"albedo","file":"color.png"}],"variants":[{"id":"small","kind":"material","pack":"one","category":"stone","name":"Stone_1K","maps":[{"role":"albedo","file":"color.png"}]}]},
            {"id":"clip","kind":"animation","standalone":true,"pack":"one","category":"animations","file":"Wave.gltf"},
            {"id":"click","kind":"audio","pack":"one","category":"sounds","file":"click.wav"}
        ]);
        let manifest = json!({"schemaVersion":1,"custom":"keep","assets":assets});
        write(
            &root,
            config::SEED_REL,
            &serde_json::to_vec(&manifest).unwrap(),
        );
        let user = json!({"favorite":true,"tags":["custom"],"collections":["Favorites"],"previewAssetId":"two","provenance":{"filesystemPath":"P:/original"}});
        let cached = json!({"schemaVersion":5,"libraryRoot":root,"rigs":[{"id":"rig","name":"Rig","kind":"humanoid","unambiguous":true,"nodes":[{"key":"hips","name":"Hips","parent":null,"rest":[-7.084689835660935e-10,-2.6706067187714407e-15,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,1.0]}]}],"assets":[{"id":"one","name":"Hero","fileName":"Hero.gltf","pack":"one","category":"characters","kind":"model","thumb":{"state":"missing"},"user":user,"relPath":"library/one/characters/Hero.gltf","fileset":{"gltf":"library/one/characters/Hero.gltf","bin":"library/one/characters/Hero.bin","textures":["library/one/textures/Atlas.png"]}}]});
        write(
            &root,
            "appdata/catalog.json",
            &serde_json::to_vec(&cached).unwrap(),
        );
        migrate(&root, &cache).unwrap();
        let after = read(&root.join(config::SEED_REL));
        let current = read(&cache);
        assert_eq!(after["custom"], "keep");
        assert_eq!(after["assets"].as_array().unwrap().len(), 5);
        assert_eq!(current["assets"][0]["user"], user);
        assert_eq!(current["rigs"], cached["rigs"]);
        let parsed = seed::parse_seed_entries(&after).unwrap();
        assert_eq!(parsed[2].variants[0].id, "small");
        for (index, folder) in [
            (0, "models"),
            (1, "models"),
            (2, "materials"),
            (3, "animations"),
            (4, "audio"),
        ] {
            assert!(parsed[index]
                .rel_path
                .starts_with(&format!("library/{folder}/")));
            assert!(root.join(&parsed[index].rel_path).exists());
        }
        assert_ne!(parsed[0].rel_path, parsed[1].rel_path);
        assert_eq!(current["assets"][0]["fileset"]["gltf"], parsed[0].rel_path);
        let moved = read(&root.join(&parsed[0].rel_path));
        assert_eq!(
            moved["extras"]["toyboxSourcePath"],
            "library/one/characters/Hero.gltf"
        );
        let uri = moved["images"][0]["uri"].as_str().unwrap();
        let texture = root
            .join(Path::new(&parsed[0].rel_path).parent().unwrap())
            .join(uri);
        assert_eq!(std::fs::read(texture).unwrap(), b"first atlas");
        assert!(!root.join("library/one").exists());
        assert!(!root.join("library/two").exists());
        let original = read(&root.join(config::SEED_REL));
        let revision = store::manifest_revision(&root).unwrap();
        migrate(&root, &cache).unwrap();
        assert_eq!(read(&root.join(config::SEED_REL)), original);
        assert_eq!(store::manifest_revision(&root).unwrap(), revision, "a second migration writes nothing");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_dependency_does_not_publish_or_delete_originals() {
        let (root, cache) = fixture("missing");
        let manifest =
            json!({"assets":[{"id":"a","pack":"p","category":"characters","file":"Hero.gltf"}]});
        write(
            &root,
            config::SEED_REL,
            &serde_json::to_vec(&manifest).unwrap(),
        );
        write(
            &root,
            "library/p/characters/Hero.gltf",
            &serde_json::to_vec(&scene("missing.png")).unwrap(),
        );
        write(&root, "library/p/characters/Hero.bin", b"mesh");
        assert!(migrate(&root, &cache).is_err());
        assert_eq!(read(&root.join(config::SEED_REL)), manifest);
        assert!(root.join("library/p/characters/Hero.bin").exists());
        assert!(!root.join(PENDING).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn interrupted_publication_resumes_and_modified_destinations_keep_originals() {
        let (root, cache) = fixture("recovery");
        let before = json!({"assets":[]});
        let after = json!({"assets":[],"storageVersion":1});
        write(
            &root,
            config::SEED_REL,
            &serde_json::to_vec(&after).unwrap(),
        );
        write(&root, "library/p/a.wav", b"original");
        write(&root, "library/audio/key/a.wav", b"corrupt");
        let transaction = Transaction {
            manifest_before: before,
            manifest_after: after.clone(),
            cache_before: None,
            cache_after: None,
            migration_before: None,
            migration_after: None,
            cache_path: cache.to_string_lossy().into(),
            files: vec![CopyFile {
                source: "library/p/a.wav".into(),
                target: "library/audio/key/a.wav".into(),
                digest: hash_bytes(b"original"),
                target_digest: hash_bytes(b"original"),
            }],
            legacy_dirs: vec![],
            archive: "_library_config/migration-backups/storage-test/unindexed".into(),
        };
        write(&root, PENDING, &serde_json::to_vec(&transaction).unwrap());
        assert!(recover(&root, &cache).is_err());
        assert!(root.join("library/p/a.wav").exists());
        write(&root, "library/audio/key/a.wav", b"original");
        recover(&root, &cache).unwrap();
        assert!(!root.join("library/p/a.wav").exists());
        assert_eq!(read(&root.join(config::SEED_REL)), after);
        recover(&root, &cache).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    /// A journal left half-done on one machine (Windows) and carried to another
    /// (a Linux server) names a cache path that means nothing there.
    fn interrupted_transaction(root: &Path, cache_path: &str) -> (Value, Value) {
        let before = json!({"assets":[]});
        let after = json!({"assets":[],"storageVersion":1});
        write(root, config::SEED_REL, &serde_json::to_vec(&after).unwrap());
        write(root, "library/p/a.wav", b"original");
        write(root, "library/audio/key/a.wav", b"original");
        let transaction = Transaction {
            manifest_before: before.clone(),
            manifest_after: after.clone(),
            cache_before: Some(json!({"schemaVersion":5,"libraryRoot":"D:/assets/Lib","assets":[]})),
            cache_after: Some(json!({"schemaVersion":5,"libraryRoot":"D:/assets/Lib","assets":[],"storageVersion":1})),
            migration_before: None,
            migration_after: None,
            cache_path: cache_path.into(),
            files: vec![CopyFile {
                source: "library/p/a.wav".into(),
                target: "library/audio/key/a.wav".into(),
                digest: hash_bytes(b"original"),
                target_digest: hash_bytes(b"original"),
            }],
            legacy_dirs: vec![],
            archive: "_library_config/migration-backups/storage-test/unindexed".into(),
        };
        write(root, PENDING, &serde_json::to_vec(&transaction).unwrap());
        (before, after)
    }
    #[test]
    fn a_journal_from_another_platform_finishes_without_its_foreign_cache() {
        let (root, cache) = fixture("foreign-journal");
        // A path that is absolute on the other kind of machine only.
        let foreign = if cfg!(windows) { "/home/ry/.local/share/toybox/libraries/abc/catalog.json" } else { "C:\\Users\\ry\\AppData\\Roaming\\com.toybox.app\\libraries\\abc\\catalog.json" };
        let (_, after) = interrupted_transaction(&root, foreign);
        recover(&root, &cache).unwrap();
        assert_eq!(read(&root.join(config::SEED_REL)), after);
        assert!(!root.join("library/p/a.wav").exists(), "the original was retired");
        assert!(root.join("library/audio/key/a.wav").exists());
        assert!(!root.join(PENDING).exists());
        assert!(!cache.exists(), "a cache that belongs to someone else is never written");
        assert!(Store::local(&root, &cache).load_catalog().unwrap().is_none(), "nor is a catalog made from it");
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn a_journal_for_a_different_cache_on_this_machine_is_still_refused() {
        let (root, cache) = fixture("other-cache");
        let (before, after) = interrupted_transaction(&root, &root.join("elsewhere/catalog.json").to_string_lossy());
        write(&root, config::SEED_REL, &serde_json::to_vec(&before).unwrap());
        assert!(recover(&root, &cache).is_err());
        assert_eq!(read(&root.join(config::SEED_REL)), before);
        assert_ne!(before, after);
        assert!(root.join("library/p/a.wav").exists() && root.join(PENDING).exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn recovery_accepts_a_library_folder_named_by_a_non_canonical_path() {
        let (root, cache) = fixture("dotted-root");
        let (_, after) = interrupted_transaction(&root, &cache.to_string_lossy());
        write(&root, "appdata/catalog.json", &serde_json::to_vec(&json!({"libraryRoot":"D:/assets/Lib","assets":[]})).unwrap());
        recover(&root.join("library").join(".."), &cache).unwrap();
        assert_eq!(read(&root.join(config::SEED_REL)), after);
        assert!(!root.join("library/p/a.wav").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn imports_are_normalized_and_cache_for_another_library_is_untouched() {
        let (root, cache) = fixture("import");
        let before = json!({"assets":[],"storageVersion":1});
        write(
            &root,
            config::SEED_REL,
            &serde_json::to_vec(&before).unwrap(),
        );
        // A catalog with nothing in it (the cache of a library root is no longer part of
        // the catalog: it belongs to this library by being in its database).
        let foreign = json!({"schemaVersion":5,"libraryRoot":"somewhere-else","assets":[]});
        write(
            &root,
            "appdata/catalog.json",
            &serde_json::to_vec(&foreign).unwrap(),
        );
        write(&root, "library/_import_test/sounds/a.wav", b"sound");
        let after = json!({"assets":[{"id":"a","kind":"audio","pack":"_import_test","category":"sounds","file":"a.wav"}],"storageVersion":1});
        publish(&root, &cache, before, after).unwrap();
        assert_eq!(read(&cache)["assets"], foreign["assets"], "the publication adds nothing to the catalog");
        assert_eq!(read(&cache)["schemaVersion"], foreign["schemaVersion"]);
        let entry = &read(&root.join(config::SEED_REL))["assets"][0];
        assert!(entry["storagePath"]
            .as_str()
            .unwrap()
            .starts_with("library/audio/"));
        assert!(!root.join("library/_import_test").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn managed_paths_reject_traversal() {
        let entry = json!({"id":"a","kind":"audio","pack":"p","category":"sounds","file":"a.wav","storagePath":"library/audio/../../outside.wav"});
        assert!(seed::parse_entry(&entry).is_err());
        let valid = json!({"kind":"audio","id":"a","file":"a.wav","storagePath":"library/audio/0123456789abcdef/a.wav"});
        assert!(seed::parse_entry(&valid).is_ok());
        let grouped = json!({"kind":"audio","id":"a","file":"a.wav","storagePath":"library/audio/My_Pack/My_Category/a.wav"});
        assert!(seed::parse_entry(&grouped).is_err());
    }
    #[test]
    fn staged_variants_commit_without_namespace_metadata_or_pack_directories() {
        let (root, cache) = fixture("variant-import");
        let before = json!({"assets":[],"storageVersion":1});
        write(
            &root,
            config::SEED_REL,
            &serde_json::to_vec(&before).unwrap(),
        );
        for name in ["Stone_2K", "Stone_1K"] {
            write(
                &root,
                &format!("library/_import_test/materials/{name}/color.png"),
                name.as_bytes(),
            );
        }
        let after = json!({"assets":[{"id":"stone","kind":"material","pack":"_import_test","category":"stone","name":"Stone_2K","maps":[{"role":"albedo","file":"color.png"}],"variants":[{"id":"small","kind":"material","pack":"_import_test","category":"stone","name":"Stone_1K","maps":[{"role":"albedo","file":"color.png"}]}]}],"storageVersion":1});
        publish(&root, &cache, before, after).unwrap();
        let manifest = read(&root.join(config::SEED_REL));
        let entries = seed::parse_seed_entries(&manifest).unwrap();
        assert_eq!(entries[0].pack, "imported");
        assert_eq!(entries[0].variants[0].pack, "imported");
        let catalog =
            crate::ops::catalog_build::build_catalog(&root.to_string_lossy(), entries)
                .unwrap();
        assert_eq!(catalog.assets[0].variants[0].id, "small");
        assert!(!root.join("library/_import_test").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_entry_keeps_the_format_it_was_imported_from_and_old_entries_load_without_one() {
        let (root, cache) = fixture("format");
        let before = json!({"assets":[],"storageVersion":1});
        write(&root, config::SEED_REL, &serde_json::to_vec(&before).unwrap());
        write(&root, "library/_import_test/materials/Tiles/albedo.png", b"albedo");
        write(&root, "library/_import_test/materials/Plain/albedo.png", b"albedo");
        let after = json!({"assets":[
            {"id":"tiles","kind":"material","pack":"_import_test","category":"tiles","name":"Tiles","maps":[{"role":"albedo","file":"albedo.png"}],"format":"sbsar"},
            {"id":"plain","kind":"material","pack":"_import_test","category":"tiles","name":"Plain","maps":[{"role":"albedo","file":"albedo.png"}]},
        ],"storageVersion":1});
        publish(&root, &cache, before, after).unwrap();
        let manifest = read(&root.join(config::SEED_REL));
        let entries = manifest["assets"].as_array().unwrap();
        let tiles = entries.iter().find(|entry| entry["id"] == "tiles").unwrap();
        let plain = entries.iter().find(|entry| entry["id"] == "plain").unwrap();
        assert_eq!(tiles["format"], "sbsar");
        assert!(plain.get("format").is_none());
        let catalog = crate::ops::catalog_build::build_catalog(&root.to_string_lossy(), seed::parse_seed_entries(&manifest).unwrap()).unwrap();
        for asset in &catalog.assets {
            let value = serde_json::to_value(asset).unwrap();
            if asset.id == "tiles" { assert_eq!(value["format"], "sbsar"); } else { assert!(value.get("format").is_none()); }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_entry_keeps_the_origin_its_importing_plugin_recorded() {
        let (root, cache) = fixture("origin");
        let before = json!({"assets":[],"storageVersion":1});
        write(&root, config::SEED_REL, &serde_json::to_vec(&before).unwrap());
        write(&root, "library/_import_test/materials/Tiles/albedo.png", b"albedo");
        write(&root, "library/_import_test/materials/Tiles/tiles.sbsar", b"archive");
        let after = json!({"assets":[
            {"id":"tiles","kind":"material","pack":"_import_test","category":"tiles","name":"Tiles","maps":[{"role":"albedo","file":"albedo.png"}],
             "origin":{"plugin":"com.example.generator","files":["tiles.sbsar"],"meta":{"source":"tiles.sbsar","preset":"Green","inputs":{"joint_height":0.4}}}},
        ],"storageVersion":1});
        publish(&root, &cache, before, after).unwrap();

        let manifest = read(&root.join(config::SEED_REL));
        assert_eq!(manifest["assets"][0]["origin"]["plugin"], "com.example.generator");
        assert_eq!(manifest["assets"][0]["origin"]["meta"]["preset"], "Green");
        let catalog = crate::ops::catalog_build::build_catalog(
            &root.to_string_lossy(),
            seed::parse_seed_entries(&manifest).unwrap(),
        )
        .unwrap();
        let origin = catalog.assets[0].origin.as_ref().expect("origin kept");
        assert_eq!(origin.plugin, "com.example.generator");
        assert_eq!(origin.meta["inputs"]["joint_height"], 0.4);
        let value = serde_json::to_value(&catalog.assets[0]).unwrap();
        assert_eq!(value["origin"]["meta"]["source"], "tiles.sbsar");

        // The source file moves into the material's own folder, next to its
        // maps: a bare name in the manifest, a library path in the catalog.
        let stored = manifest["assets"][0]["storagePath"].as_str().unwrap().to_string();
        assert_eq!(manifest["assets"][0]["origin"]["files"], json!(["tiles.sbsar"]));
        assert_eq!(origin.files, vec![format!("{stored}/tiles.sbsar")]);
        assert_eq!(std::fs::read(root.join(&origin.files[0])).unwrap(), b"archive");
        assert_eq!(std::fs::read(root.join(format!("{stored}/albedo.png"))).unwrap(), b"albedo");
        assert!(!root.join("library/_import_test").exists());
        // A different archive is a different material folder.
        let again = json!({"assets":[
            {"id":"tiles","kind":"material","pack":"_import_two","category":"tiles","name":"Tiles","maps":[{"role":"albedo","file":"albedo.png"}],
             "origin":{"plugin":"com.example.generator","files":["tiles.sbsar"]}},
        ],"storageVersion":1});
        write(&root, "library/_import_two/materials/Tiles/albedo.png", b"albedo");
        write(&root, "library/_import_two/materials/Tiles/tiles.sbsar", b"another archive");
        publish(&root, &cache, manifest.clone(), again).unwrap();
        let updated = read(&root.join(config::SEED_REL));
        assert_ne!(updated["assets"][0]["storagePath"], manifest["assets"][0]["storagePath"]);

        // Names must be plain, distinct from the maps, and only materials keep them.
        for bad in [
            json!({"id":"m","kind":"material","pack":"p","category":"c","name":"M","maps":[{"role":"albedo","file":"a.png"}],"origin":{"plugin":"x","files":["../a.sbsar"]}}),
            json!({"id":"m","kind":"material","pack":"p","category":"c","name":"M","maps":[{"role":"albedo","file":"a.png"}],"origin":{"plugin":"x","files":["A.PNG"]}}),
            json!({"id":"m","kind":"material","pack":"p","category":"c","name":"M","maps":[{"role":"albedo","file":"a.png"}],"origin":{"plugin":"x","files":["s.sbsar","S.sbsar"]}}),
            json!({"id":"s","kind":"audio","pack":"p","category":"c","file":"s.wav","origin":{"plugin":"x","files":["s.proj"]}}),
        ] {
            assert!(seed::parse_entry(&bad).is_err(), "{bad}");
        }

        // An entry without one, or with a malformed one, simply has none.
        use crate::domain::catalog_model::AssetOrigin;
        assert_eq!(AssetOrigin::from_entry(&json!({"id":"a"})), None);
        assert_eq!(AssetOrigin::from_entry(&json!({"origin":{"plugin":"  "}})), None);
        assert_eq!(AssetOrigin::from_entry(&json!({"origin":"com.example"})), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_model_and_its_variants_keep_the_original_files_beside_their_gltf() {
        let (root, cache) = fixture("model-origin");
        let before = json!({"assets":[],"storageVersion":1});
        write(&root, config::SEED_REL, &serde_json::to_vec(&before).unwrap());
        for (stem, original) in [("Crate", "full original"), ("Crate_LOD1", "low original")] {
            write(&root, &format!("library/_import_test/props/{stem}.gltf"),
                &serde_json::to_vec(&json!({"asset":{"version":"2.0"},"buffers":[{"uri":format!("{stem}.bin"),"byteLength":4}],"images":[{"uri":"wood.png"}]})).unwrap());
            write(&root, &format!("library/_import_test/props/{stem}.bin"), b"mesh");
            write(&root, &format!("library/_import_test/props/{stem}.obj"), original.as_bytes());
        }
        write(&root, "library/_import_test/props/wood.png", b"wood");
        // Both models name the same material file; it is staged once.
        write(&root, "library/_import_test/props/shared.mtl", b"materials");
        let origin = |stem: &str| json!({"plugin":"com.example.obj","files":[format!("{stem}.obj"),"shared.mtl"],
            "meta":{"format":"obj","source":format!("models/{stem}.obj"),"companions":{"shared.mtl":"shared.mtl"}}});
        let after = json!({"assets":[
            {"id":"crate","pack":"_import_test","category":"props","file":"Crate.gltf","format":"obj","origin":origin("Crate"),
             "variants":[{"id":"crate1","pack":"_import_test","category":"props","file":"Crate_LOD1.gltf","format":"obj","origin":origin("Crate_LOD1")}]},
        ],"storageVersion":1});
        publish(&root, &cache, before, after).unwrap();

        // The manifest keeps bare names; the catalog gets library paths in each model's own folder.
        let manifest = read(&root.join(config::SEED_REL));
        assert_eq!(manifest["assets"][0]["origin"]["files"], json!(["Crate.obj", "shared.mtl"]));
        assert_eq!(manifest["assets"][0]["variants"][0]["origin"]["files"], json!(["Crate_LOD1.obj", "shared.mtl"]));
        let catalog = crate::ops::catalog_build::build_catalog(&root.to_string_lossy(), seed::parse_seed_entries(&manifest).unwrap()).unwrap();
        let full = &catalog.assets[0];
        let low = &full.variants[0];
        let folder = |rel: &str| rel.rsplit_once('/').unwrap().0.to_string();
        let full_folder = folder(&full.rel_path);
        let low_folder = folder(&low.rel_path);
        assert_ne!(full_folder, low_folder);
        let kept = full.origin.as_ref().unwrap();
        assert_eq!(kept.plugin, "com.example.obj");
        assert_eq!(kept.meta["source"], "models/Crate.obj");
        assert_eq!(kept.files, vec![format!("{full_folder}/Crate.obj"), format!("{full_folder}/shared.mtl")]);
        assert_eq!(low.origin.as_ref().unwrap().files, vec![format!("{low_folder}/Crate_LOD1.obj"), format!("{low_folder}/shared.mtl")]);
        assert_eq!(std::fs::read(root.join(&kept.files[0])).unwrap(), b"full original");
        assert_eq!(std::fs::read(root.join(&low.origin.as_ref().unwrap().files[0])).unwrap(), b"low original");
        assert_eq!(std::fs::read(root.join(&kept.files[1])).unwrap(), b"materials");
        assert!(root.join(&full.rel_path).exists());
        assert!(!root.join("library/_import_test").exists());
        // A different original is a different folder.
        let again_before = manifest.clone();
        write(&root, "library/_import_two/props/Crate.gltf", &serde_json::to_vec(&json!({"asset":{"version":"2.0"},"buffers":[{"uri":"Crate.bin","byteLength":4}]})).unwrap());
        write(&root, "library/_import_two/props/Crate.bin", b"mesh");
        write(&root, "library/_import_two/props/Crate.obj", b"edited original");
        let again = json!({"assets":[{"id":"crate","pack":"_import_two","category":"props","file":"Crate.gltf","format":"obj","origin":{"plugin":"com.example.obj","files":["Crate.obj"]}}],"storageVersion":1});
        publish(&root, &cache, again_before.clone(), again).unwrap();
        assert_ne!(read(&root.join(config::SEED_REL))["assets"][0]["storagePath"], again_before["assets"][0]["storagePath"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn kept_files_of_a_model_must_be_plain_distinct_names_that_leave_the_gltf_and_bin_alone() {
        let model = |files: Value| json!({"id":"m","pack":"p","category":"c","file":"Crate.gltf","origin":{"plugin":"x","files":files}});
        assert!(seed::parse_entry(&model(json!(["Crate.obj", "Crate.mtl"]))).is_ok());
        for bad in [json!(["Crate.gltf"]), json!(["crate.BIN"]), json!(["a.obj", "A.OBJ"]), json!(["../a.obj"]), json!(["sub/a.obj"]), json!([""])] {
            assert!(seed::parse_entry(&model(bad.clone())).is_err(), "{bad}");
        }
        // An animation source is replaced by its clips, so it keeps nothing.
        let animation = json!({"id":"a","kind":"animation","pack":"p","category":"animations","file":"Walk.gltf","origin":{"plugin":"x","files":["Walk.fbx"]}});
        assert!(seed::parse_entry(&animation).is_err());
        let plain = json!({"id":"a","kind":"animation","pack":"p","category":"animations","file":"Walk.gltf","origin":{"plugin":"x"}});
        assert!(seed::parse_entry(&plain).is_ok());
    }

    #[test]
    fn plugin_defined_assets_commit_to_their_type_folder_and_round_trip() {
        use crate::domain::catalog_model::AssetContent;
        let (root, cache) = fixture("plugin-type");
        let before = json!({"assets":[],"storageVersion":1});
        write(&root, config::SEED_REL, &serde_json::to_vec(&before).unwrap());
        // A cube map is six files that must keep their names; a panorama is one.
        for face in ["px.png", "nx.png"] {
            write(&root, &format!("library/_import_test/skybox/Night/{face}"), face.as_bytes());
        }
        write(&root, "library/_import_test/skybox/Noon/noon.hdr", b"radiance");
        let after = json!({"assets":[
            {"id":"night","kind":"plugin","type":"skybox","pack":"_import_test","category":"night","name":"Night","files":["px.png","nx.png"],"meta":{"layout":"cubemap","faces":{"px":"px.png","nx":"nx.png"}}},
            {"id":"noon","kind":"plugin","type":"skybox","pack":"_import_test","category":"day","name":"Noon","files":["noon.hdr"]}
        ],"storageVersion":1});
        publish(&root, &cache, before, after).unwrap();

        let manifest = read(&root.join(config::SEED_REL));
        let stored = manifest["assets"][0]["storagePath"].as_str().unwrap().to_string();
        assert!(stored.starts_with("library/skybox/") && stored.split('/').count() == 3);
        assert_eq!(manifest["assets"][0]["files"], json!(["px.png", "nx.png"]));
        assert_eq!(manifest["assets"][0]["pack"], "imported");
        assert_eq!(manifest["assets"][0]["meta"]["faces"]["px"], "px.png");
        assert!(!root.join("library/_import_test").exists());

        let catalog = crate::ops::catalog_build::build_catalog(
            &root.to_string_lossy(),
            seed::parse_seed_entries(&manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(catalog.assets.len(), 2);
        let AssetContent::Plugin { type_id, files, meta } = &catalog.assets[0].content else { panic!("plugin asset expected") };
        assert_eq!(type_id, "skybox");
        assert_eq!(files, &vec![format!("{stored}/px.png"), format!("{stored}/nx.png")]);
        assert_eq!(meta["layout"], "cubemap");
        assert_eq!(catalog.assets[0].rel_path, stored);
        assert_eq!(std::fs::read(root.join(&files[0])).unwrap(), b"px.png");

        // The cached catalog keeps the generic shape and reloads as the same type.
        let value = serde_json::to_value(&catalog.assets[1]).unwrap();
        assert_eq!(value["kind"], "plugin");
        assert_eq!(value["type"], "skybox");
        assert_eq!(value["meta"], json!({}));
        let back: crate::domain::catalog_model::Asset = serde_json::from_value(value).unwrap();
        assert_eq!(back.content.type_id(), "skybox");

        // A missing file fails indexing instead of listing a broken item.
        std::fs::remove_file(root.join(&files[1])).unwrap();
        assert!(crate::ops::catalog_build::build_catalog(
            &root.to_string_lossy(),
            seed::parse_seed_entries(&manifest).unwrap(),
        )
        .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plugin_entries_reject_bad_type_ids_paths_and_file_lists() {
        let entry = |patch: Value| {
            let mut value = json!({"id":"a","kind":"plugin","type":"skybox","pack":"p","category":"c","name":"Sky","files":["sky.hdr"]});
            for (key, item) in patch.as_object().unwrap() {
                value[key] = item.clone();
            }
            seed::parse_entry(&value)
        };
        assert!(entry(json!({})).is_ok());
        // Ids are lower-case words and cannot shadow the host's own folders.
        for id in ["Skybox", "sky box", "a", "models", "audio", "plugin", "../x", "9lives"] {
            assert!(entry(json!({"type":id})).is_err(), "{id}");
        }
        for files in [json!([]), json!(["a/b.hdr"]), json!([".."]), json!(["sky.hdr", "SKY.HDR"]), json!([3]), json!("sky.hdr")] {
            assert!(entry(json!({"files":files})).is_err());
        }
        assert!(entry(json!({"name":"a/b"})).is_err());
        assert!(entry(json!({"meta":[1]})).is_err());
        // Managed locations must sit directly in the type's own folder.
        let key = "0123456789abcdef";
        assert!(entry(json!({"storagePath":format!("library/skybox/{key}")})).is_ok());
        assert!(entry(json!({"storagePath":format!("library/audio/{key}")})).is_err());
        assert!(entry(json!({"storagePath":format!("library/skybox/{key}/sky.hdr")})).is_err());
        assert!(entry(json!({"storagePath":"library/skybox/../../outside"})).is_err());
    }

    fn snapshots(root: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(root.join("_library_config/migration-backups"))
            .map(|read| read.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("storage-")).collect())
            .unwrap_or_default();
        found.sort();
        found
    }
    /// `tidy` declines while another test of this process publishes; wait for its turn.
    fn tidy_when_idle(root: &Path) {
        for _ in 0..500 {
            if tidy(root) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the library stayed busy");
    }
    /// One more import published through the real transaction (new content each time).
    fn import_one(root: &Path, cache: &Path, index: usize) {
        let before = read(&root.join(config::SEED_REL));
        let mut after = before.clone();
        let pack = format!("_import_t{index}");
        write(root, &format!("library/{pack}/sounds/a.wav"), format!("sound {index}").as_bytes());
        after["assets"].as_array_mut().unwrap().push(json!({"id":format!("a{index}"),"kind":"audio","pack":pack,"category":"sounds","file":"a.wav"}));
        publish(root, cache, before, after).unwrap();
    }
    #[test]
    fn publications_keep_only_the_newest_snapshots_and_a_slim_record() {
        let (root, cache) = fixture("prune");
        write(&root, config::SEED_REL, &serde_json::to_vec(&json!({"assets":[],"storageVersion":1})).unwrap());
        for index in 0..retention::KEEP_STORAGE_SNAPSHOTS + 3 {
            import_one(&root, &cache, index);
        }
        let kept = snapshots(&root);
        assert_eq!(kept.len(), retention::KEEP_STORAGE_SNAPSHOTS, "{kept:?}");
        // The newest publication's snapshot is among them, with its record and the before-copies.
        let newest = root.join("_library_config/migration-backups").join(kept.last().unwrap());
        assert!(newest.join("manifest.before.json").exists());
        let record = read(&newest.join("transaction.json"));
        assert_eq!(record["files"].as_array().unwrap().len(), 1);
        assert!(record["manifest_before"].as_str().unwrap().starts_with("omitted"));
        // A schema-migration backup and an unrelated folder are never touched.
        std::fs::create_dir_all(root.join("_library_config/migration-backups/0001-123")).unwrap();
        std::fs::create_dir_all(root.join("_library_config/migration-backups/storage-test")).unwrap();
        import_one(&root, &cache, 99);
        assert_eq!(snapshots(&root).len(), retention::KEEP_STORAGE_SNAPSHOTS + 1, "storage-test is not a stamp and stays");
        assert!(root.join("_library_config/migration-backups/0001-123").is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }
    fn old_snapshots(root: &Path, count: u32) {
        for stamp in 1..=count {
            write(root, &format!("_library_config/migration-backups/storage-{stamp}/manifest.before.json"), b"{}");
        }
    }
    #[test]
    fn the_snapshot_of_an_interrupted_publication_survives_pruning_and_recovery_still_works() {
        let (root, cache) = fixture("crash-prune");
        old_snapshots(&root, 8);
        // The interrupted publication is the OLDEST snapshot, so plain pruning would remove it first.
        let (_, after) = interrupted_transaction(&root, &cache.to_string_lossy());
        let mut journal = read(&root.join(PENDING));
        journal["archive"] = json!("_library_config/migration-backups/storage-1/unindexed");
        journal["legacy_dirs"] = json!(["library/_import_left"]);
        write(&root, PENDING, &serde_json::to_vec(&journal).unwrap());
        write(&root, "library/_import_left/notes.txt", b"extra source file");
        write(&root, "appdata/catalog.json", &serde_json::to_vec(&json!({"libraryRoot":"D:/assets/Lib","assets":[]})).unwrap());
        tidy_when_idle(&root);
        let kept = snapshots(&root);
        assert!(kept.contains(&"storage-1".to_string()), "{kept:?}");
        assert_eq!(kept.len(), retention::KEEP_STORAGE_SNAPSHOTS + 1, "five newest plus the protected one: {kept:?}");
        assert!(root.join(PENDING).exists(), "tidying never finishes or drops the journal");
        // Recovery after the "crash" completes against the preserved snapshot folder.
        recover(&root, &cache).unwrap();
        assert_eq!(read(&root.join(config::SEED_REL)), after);
        assert!(!root.join("library/p/a.wav").exists() && root.join("library/audio/key/a.wav").exists());
        assert!(!root.join(PENDING).exists());
        let archived = root.join("_library_config/migration-backups/storage-1/unindexed/_import_left/notes.txt");
        assert_eq!(std::fs::read(&archived).unwrap(), b"extra source file");
        // That snapshot now holds the user's archived files, so later pruning leaves it alone.
        tidy_when_idle(&root);
        assert!(archived.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn an_interrupted_snapshot_without_archived_files_is_pruned_once_recovery_has_finished() {
        let (root, cache) = fixture("crash-prune-plain");
        old_snapshots(&root, 7);
        let (_, after) = interrupted_transaction(&root, &cache.to_string_lossy());
        let mut journal = read(&root.join(PENDING));
        journal["archive"] = json!("_library_config/migration-backups/storage-1/unindexed");
        write(&root, PENDING, &serde_json::to_vec(&journal).unwrap());
        write(&root, "appdata/catalog.json", &serde_json::to_vec(&json!({"libraryRoot":"D:/assets/Lib","assets":[]})).unwrap());
        tidy_when_idle(&root);
        assert!(snapshots(&root).contains(&"storage-1".to_string()));
        recover(&root, &cache).unwrap();
        assert_eq!(read(&root.join(config::SEED_REL)), after);
        tidy_when_idle(&root);
        assert_eq!(snapshots(&root), ["storage-3", "storage-4", "storage-5", "storage-6", "storage-7"]);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn tidy_removes_empty_set_folders_and_skips_while_a_publication_holds_the_lock() {
        let (root, _) = fixture("tidy");
        let key = "ab".repeat(32);
        std::fs::create_dir_all(root.join("_library_config")).unwrap();
        std::fs::create_dir_all(root.join("library/audio").join(&key)).unwrap();
        {
            let _busy = LOCK.lock().unwrap();
            assert!(!tidy(&root), "a publication in progress is never disturbed");
            assert!(root.join("library/audio").join(&key).exists());
        }
        tidy_when_idle(&root);
        assert!(!root.join("library/audio").join(&key).exists());
        assert!(root.join("library/audio").is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }
    /// Journal a publication of one sound the way `ItemPlan::commit` does, then stop
    /// before anything is put into the database: what a crash at that point leaves.
    fn journaled_sound(root: &Path, cache: &Path) -> (ItemsTransaction, Value) {
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36u32 + 200).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(8000u32.to_le_bytes());
        wav.extend(16000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(200u32.to_le_bytes());
        wav.extend(vec![0u8; 200]);
        write(root, config::SEED_REL, &serde_json::to_vec(&json!({"assets":[],"storageVersion":1})).unwrap());
        write(root, "library/_import_x/sfx/click.wav", &wav);
        let entry = json!({"id":"uid://click","kind":"audio","pack":"_import_x","category":"sfx","file":"click.wav"});
        let mut transaction = exclusive(root, cache, |canonical| {
            let mut plan = ItemPlan::new(canonical, vec![entry.clone()])?;
            plan.place()?;
            let stored = plan.entries()[0].clone();
            let built = crate::ops::catalog_build::build_catalog(&canonical.to_string_lossy(), vec![seed::parse_entry(&stored)?])?;
            let delta = Delta {
                puts: vec![store::ItemPut { id: "uid://click".into(), entry: Some(stored), assets: built.assets, rigs: built.rigs }],
                storage_version: Some(LAYOUT),
                ..Default::default()
            };
            Ok(ItemsTransaction {
                delta,
                before: BTreeMap::new(),
                files: plan.plan.files.values().cloned().collect(),
                legacy_dirs: vec!["library/_import_x".into()],
                archive: "_library_config/migration-backups/storage-test/unindexed".into(),
            })
        })
        .unwrap();
        transaction.delta.expect_manifest_revision = None;
        let stored = transaction.delta.puts[0].entry.clone().unwrap();
        write(root, PENDING_ITEMS, &serde_json::to_vec(&transaction).unwrap());
        (transaction, stored)
    }
    #[test]
    fn an_interrupted_publication_of_items_finishes_from_its_journal_alone() {
        let (root, cache) = fixture("items-journal");
        let (_, stored) = journaled_sound(&root, &cache);
        let managed = root.join(stored["storagePath"].as_str().unwrap());
        assert!(managed.is_file() && root.join("library/_import_x/sfx/click.wav").is_file());
        assert!(Store::local(&root, &cache).load_entries(&["uid://click".to_string()]).unwrap().is_empty(), "nothing is registered yet");
        recover(&root, &cache).unwrap();
        assert_eq!(Store::local(&root, &cache).load_entries(&["uid://click".to_string()]).unwrap(), vec![stored]);
        assert_eq!(Store::local(&root, &cache).load_top_assets(&["uid://click".to_string()]).unwrap().len(), 1, "its derived rows came with it");
        assert!(!root.join(PENDING_ITEMS).exists(), "the journal is done");
        assert!(!root.join("library/_import_x").exists(), "the staged original was retired");
        assert!(managed.is_file());
        let revision = store::manifest_revision(&root).unwrap();
        recover(&root, &cache).unwrap();
        assert_eq!(store::manifest_revision(&root).unwrap(), revision, "finishing twice writes nothing");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn a_journal_of_items_is_refused_when_the_library_was_edited_meanwhile() {
        let (root, cache) = fixture("items-journal-edited");
        let (_, stored) = journaled_sound(&root, &cache);
        // Someone else registered the same id with different content after the journal was written.
        let mut other = stored.clone();
        other["category"] = json!("elsewhere");
        let store = Store::local(&root, &cache);
        store.apply(&Delta { puts: vec![store::ItemPut { id: "uid://click".into(), entry: Some(other), assets: vec![], rigs: vec![] }], skip_catalog: true, ..Default::default() }).unwrap();
        assert!(recover(&root, &cache).is_err());
        assert!(root.join(PENDING_ITEMS).exists(), "the journal is kept for a person to look at");
        assert!(root.join("library/_import_x/sfx/click.wav").is_file(), "no original is touched");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn a_failed_placement_removes_the_files_it_made() {
        let (root, _) = fixture("items-discard");
        std::fs::create_dir_all(root.join("_library_config")).unwrap();
        write(&root, "library/_import_x/sfx/a.wav", b"one");
        write(&root, "library/_import_x/sfx/b.wav", b"two");
        let entries = vec![
            json!({"id":"a","kind":"audio","pack":"_import_x","category":"sfx","file":"a.wav"}),
            json!({"id":"b","kind":"audio","pack":"_import_x","category":"sfx","file":"b.wav"}),
        ];
        let mut plan = ItemPlan::new(&root, entries).unwrap();
        plan.place().unwrap();
        let made: Vec<_> = plan.created.clone();
        assert_eq!(made.len(), 2);
        assert!(made.iter().all(|path| path.is_file()));
        plan.discard();
        assert!(made.iter().all(|path| !path.exists()), "what was copied is gone");
        assert!(root.join("library/_import_x/sfx/a.wav").is_file(), "the staged originals stay");
        assert!(!root.join("library/audio").exists() || std::fs::read_dir(root.join("library/audio")).unwrap().next().is_none(), "and so do no empty set folders");
        std::fs::remove_dir_all(root).unwrap();
    }
}
