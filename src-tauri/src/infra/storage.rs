//! Managed, immutable file sets grouped only by asset type. Pack/category are
//! metadata, never locations. Copy and verify before publishing either catalog;
//! a durable journal makes interrupted publication/cleanup resumable.
use crate::{
    config,
    domain::{
        paths,
        seed::{self, SeedContent},
    },
    error::{AppError, AppResult},
    infra::fsio,
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

pub fn type_folder(kind: &str) -> AppResult<&'static str> {
    match kind {
        "model" => Ok("models"),
        "material" => Ok("materials"),
        "animation" => Ok("animations"),
        "audio" => Ok("audio"),
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
        let type_root = format!("library/{}", type_folder(&kind)?);
        // The immutable set key includes identity, declaration and file contents.
        // Updating an item publishes new paths instead of modifying live files.
        let mut key = Sha256::new();
        key.update(parsed.id.as_bytes());
        key.update(serde_json::to_vec(entry)?);
        let mut doc = None;
        let mut dependencies = Vec::new();
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
                            if uri.starts_with("data:") {
                                continue;
                            }
                            if uri.contains(':') || uri.starts_with('/') || uri.contains('\\') {
                                return Err(AppError::msg(
                                    "External scene URI is not library media",
                                ));
                            }
                            let decoded = decode_uri(uri)?;
                            let source = paths::resolve_uri_rel(&parent, &decoded);
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

fn decode_uri(uri: &str) -> AppResult<String> {
    let mut bytes = Vec::new();
    let mut chars = uri.as_bytes().iter().copied();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let a = chars.next().and_then(|b| (b as char).to_digit(16));
            let b = chars.next().and_then(|b| (b as char).to_digit(16));
            match (a, b) {
                (Some(a), Some(b)) => bytes.push((a * 16 + b) as u8),
                _ => return Err(AppError::msg("Invalid scene URI escape")),
            }
        } else {
            bytes.push(byte)
        }
    }
    String::from_utf8(bytes).map_err(|_| AppError::msg("Invalid UTF-8 scene URI"))
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
    let before: Value = serde_json::from_slice(&fsio::read_bytes(&root.join(config::SEED_REL))?)?;
    let after = before.clone();
    publish_inner(&root, cache, before, after)
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
    let old_cache = if cache.exists() {
        let doc: Value = serde_json::from_slice(&fsio::read_bytes(cache)?)?;
        if doc["libraryRoot"]
            .as_str()
            .is_some_and(|s| Path::new(s).canonicalize().ok().as_ref() == Some(&root))
        {
            Some(doc)
        } else {
            None
        }
    } else {
        None
    };
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
    let current: Value = serde_json::from_slice(&fsio::read_bytes(&root.join(config::SEED_REL))?)?;
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
    fsio::write_bytes(
        &backup.join("manifest.before.json"),
        &fsio::read_bytes(&root.join(config::SEED_REL))?,
    )?;
    if old_cache.is_some() {
        fsio::write_bytes(&backup.join("cache.before.json"), &fsio::read_bytes(cache)?)?;
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
    fsio::write_bytes(
        &backup.join("transaction.json"),
        &serde_json::to_vec_pretty(&transaction)?,
    )?;
    fsio::write_bytes_atomic(
        &root.join(PENDING),
        &serde_json::to_vec_pretty(&transaction)?,
    )?;
    finish(&root, cache, transaction)
}

pub fn recover(root: &Path, cache: &Path) -> AppResult<()> {
    let _guard = LOCK
        .lock()
        .map_err(|_| AppError::msg("Storage migration lock was poisoned"))?;
    let _process_guard = process_lock(root)?;
    recover_inner(root, cache)
}
fn recover_inner(root: &Path, cache: &Path) -> AppResult<()> {
    let path = root.join(PENDING);
    if !path.exists() {
        return Ok(());
    }
    let transaction: Transaction = serde_json::from_slice(&fsio::read_bytes(&path)?)?;
    if Path::new(&transaction.cache_path) != cache {
        return Err(AppError::msg(
            "Storage recovery requires its original catalog cache",
        ));
    }
    finish(root, cache, transaction)
}

fn finish(root: &Path, cache: &Path, transaction: Transaction) -> AppResult<()> {
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
    let manifest_path = root.join(config::SEED_REL);
    let current: Value = serde_json::from_slice(&fsio::read_bytes(&manifest_path)?)?;
    if current != transaction.manifest_before && current != transaction.manifest_after {
        return Err(AppError::msg(
            "Storage recovery stopped: manifest was edited externally",
        ));
    }
    if let Some(after) = &transaction.cache_after {
        let current = if cache.exists() {
            Some(serde_json::from_slice::<Value>(&fsio::read_bytes(cache)?)?)
        } else {
            None
        };
        if current != transaction.cache_before && current.as_ref() != Some(after) {
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
    fsio::write_bytes_atomic(
        &manifest_path,
        &serde_json::to_vec_pretty(&transaction.manifest_after)?,
    )?;
    if let Some(after) = &transaction.cache_after {
        fsio::write_bytes_atomic(cache, &serde_json::to_vec_pretty(after)?)?;
    }
    if let Some(after) = &transaction.migration_after {
        fsio::write_bytes_atomic(
            &root.join("_library_config/migrations.json"),
            &serde_json::to_vec_pretty(after)?,
        )?;
    }
    // Only retire explicitly migrated files, never recursively remove a pack.
    // Extra source files and unknown metadata are left untouched.
    let destinations: HashSet<_> = transaction
        .files
        .iter()
        .map(|file| file.target.as_str())
        .collect();
    let mut removed = HashSet::new();
    for file in &transaction.files {
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
    if !transaction
        .archive
        .starts_with("_library_config/migration-backups/storage-")
        || Path::new(&transaction.archive)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(AppError::msg("Invalid storage backup location"));
    }
    let archive = root.join(&transaction.archive);
    for relative in &transaction.legacy_dirs {
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
    fsio::remove_file_if_exists(&root.join(PENDING))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> (PathBuf, PathBuf) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("toybox-storage-{name}-{stamp}"));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let cache = root.join("appdata/catalog.json");
        (root, cache)
    }
    fn write(root: &Path, path: &str, bytes: &[u8]) {
        fsio::write_bytes(&root.join(path), bytes).unwrap();
    }
    fn read(path: &Path) -> Value {
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
        let cached = json!({"schemaVersion":5,"libraryRoot":root,"rigs":[{"id":"rig","custom":"keep","rest":[-7.084689835660935e-10,-2.6706067187714407e-15]}],"assets":[{"id":"one","user":user,"relPath":"library/one/characters/Hero.gltf","fileset":{"gltf":"library/one/characters/Hero.gltf","bin":"library/one/characters/Hero.bin","textures":["library/one/textures/Atlas.png"]}}]});
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
        let original = fsio::read_bytes(&root.join(config::SEED_REL)).unwrap();
        migrate(&root, &cache).unwrap();
        assert_eq!(
            fsio::read_bytes(&root.join(config::SEED_REL)).unwrap(),
            original
        );
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
    #[test]
    fn imports_are_normalized_and_cache_for_another_library_is_untouched() {
        let (root, cache) = fixture("import");
        let before = json!({"assets":[],"storageVersion":1});
        write(
            &root,
            config::SEED_REL,
            &serde_json::to_vec(&before).unwrap(),
        );
        let foreign = json!({"libraryRoot":"somewhere-else","assets":[],"keep":true});
        write(
            &root,
            "appdata/catalog.json",
            &serde_json::to_vec(&foreign).unwrap(),
        );
        write(&root, "library/_import_test/sounds/a.wav", b"sound");
        let after = json!({"assets":[{"id":"a","kind":"audio","pack":"_import_test","category":"sounds","file":"a.wav"}],"storageVersion":1});
        publish(&root, &cache, before, after).unwrap();
        assert_eq!(read(&cache), foreign);
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
            crate::commands::catalog_build::build_catalog(&root.to_string_lossy(), entries)
                .unwrap();
        assert_eq!(catalog.assets[0].variants[0].id, "small");
        assert!(!root.join("library/_import_test").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
