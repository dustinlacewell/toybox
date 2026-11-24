//! Library migration step 6, `substance-assets`: every Substance archive (`.sbsar`) a material keeps as
//! a shared source (`library/materials/_sources/<sha256>/<name>.sbsar`, step 5) becomes an asset of its
//! own, a *substance* (plugin type `substance` of `com.toybox.material-sbsar`), and the materials rendered
//! from it link to it with `origin.derivedFrom` instead of listing the archive in `origin.files`.
//!
//! One archive at a time ("a group": the archive and every material and variant that lists it):
//!
//! 1. The substance entry is planned: id `uid://substance_<sha256[..32]>`, storage folder
//!    `library/substance/<key>` (the key computed by [`storage::planned_folder`], the code a normal commit
//!    uses, so the same archive imported later lands in the same folder), files `[<archive>, preview.png]`
//!    (the preview, a downscale of the first material's albedo map, only when that map can be read).
//! 2. The journal `_library_config/substance-pending.json` is written (atomically) naming where the
//!    archive is, where it goes, the files this step creates and the substance's storage path. It is
//!    never written while one is there: an unsettled journal stops the run.
//! 3. The preview is written and the archive is given a **second name** in the substance's folder: a hard
//!    link (same volume: no copy, no extra space, so a nearly full disk converts). The `_sources` name
//!    stays. Where hard links are not supported the archive is renamed instead (only when nothing but this
//!    group lists it), and where that fails too it is copied.
//! 4. The archive is read once and its sha256 compared with the name of its `_sources` folder; on a
//!    mismatch the group is undone and skipped (its materials keep their archive), so the content hash a
//!    substance records is always true. (A copy is verified by the same single read of the copy.)
//! 5. One publication ([`ItemPlan`]: its own journal, then one database transaction) puts the substance
//!    and rewrites the materials together: the archive's path leaves `origin.files`, `origin.derivedFrom`
//!    is set, `format: "sbsar"` is added where it was missing. Nothing else of a material changes: not its
//!    id, folder, maps, tags, personal data or thumbnail (stored items are never keyed again).
//! 6. The `_sources` name is removed, when nothing else lists it, and the journal goes. A failure of
//!    this cleanup is a warning (the extra name is harmless: it is the same file, or a verified copy).
//!
//! A material made from two archives (two `.sbsar` in `origin.files`) is linked to the first one in
//! `origin.files` order, whichever group runs first; the others still become substances (nothing is left
//! in `_sources` unlisted) and leave its `origin.files`; each such link is reported (`extraLinks`).
//!
//! Invariants, which make every interruption recoverable the same way (`recover`):
//!
//! * The archive is present at every path any entry names, at every moment: before the commit the
//!   entries name the `_sources` path (still there: a link only adds a name), after it the substance
//!   names its own folder (linked or copied before the commit). Rolling back is deleting the added name;
//!   nothing ever has to be moved back — except on a file system without hard links, where the rename
//!   fallback is undone by renaming back.
//! * The substance and the material rewrites are one transaction, so there is no state with a substance
//!   whose materials still list the archive, or materials linked to a substance that is not there.
//! * The step never decides from the database while a storage publication journal
//!   (`storage-pending-items.json`) can still be replayed: `recover` finishes that journal first (inside
//!   the storage lock), and if it cannot be finished the run stops with both journals in place. The next
//!   open finishes the storage journal ([`storage::recover`], from `migrations::run`) and only then runs
//!   this step's `recover`.
//! * A journal is removed only after the outcome is settled; a `recover` that fails (a rename back
//!   refused, say) stops the run with the journal in place, the open goes on (logged), and the next open
//!   tries again.
//! * The `_sources` name is removed only on proof that nothing else lists it: a scan of the entries
//!   made under the same lock, never the derived file rows alone (which are stale when the catalog is not
//!   current). Where there is no such proof the name is kept; an unlisted `_sources` name of a converted
//!   archive is the same file as the substance's (or a verified copy), so `tbx library check --fix` may
//!   remove it as an orphan without losing anything. Nothing treats the two names as independent copies.
//! * Every step runs inside [`storage::exclusive`]: one writer per library across processes. A second
//!   process that finds the lock taken does nothing and leaves the work to the next open.
//!
//! A substance that is there already (an older client committed an old-style material through a newer
//! server, or the same archive was imported again): its materials are linked and the duplicate `_sources`
//! copy is removed after checking it is the same content and nothing lists it. A group that fails (the
//! archive is missing or not what its name says, a file cannot be placed) is skipped and reported, and
//! remembered with the archive's size and modification time: it is tried again when those change or a
//! new Toybox version runs, not on every open or write. Materials with other kinds of kept sources, and
//! models, are not touched: `_sources` stays the general mechanism for those.
//!
//! An import that replaces the only material listing an archive whose substance does not exist yet with
//! a material derived from that substance (an old client's straggler, or a skipped group) converts the
//! archive first ([`convert_before_replacement`]), so the import never deletes the only copy.
//!
//! State in the library database (`meta`): `substanceAssets` (`done` once the step ran),
//! `substanceAssetsReport` (JSON, [`SubstanceMigration`]) and `substanceAssetsChecked` (the manifest
//! revision at which nothing was left to do, so an open or a write does not look again until something
//! is written: the common path is one `meta` read).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::domain::{paths, seed, sources};
use crate::error::{AppError, AppResult};
use crate::infra::storage::{self, ItemPlan};
use crate::infra::store::{Delta, ItemPut, Store};
use crate::infra::{fsio, retention};

/// The asset type a substance is stored as (a plugin-defined type).
pub const TYPE_ID: &str = "substance";
/// The plugin a substance records when its materials name none.
pub const DEFAULT_PLUGIN: &str = "com.toybox.material-sbsar";
/// The file name of a substance's preview image.
pub const PREVIEW: &str = "preview.png";
/// The largest side of the preview, in pixels.
const PREVIEW_SIZE: u32 = 512;
pub const META_DONE: &str = "substanceAssets";
pub const META_REPORT: &str = "substanceAssetsReport";
const META_CHECKED: &str = "substanceAssetsChecked";
const PENDING: &str = "_library_config/substance-pending.json";
/// Entries read per page while looking for archives.
const PAGE: usize = 200;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the step did, over every run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubstanceMigration {
    /// Substance assets created.
    pub substances: usize,
    /// Material items (with their variants) now linked to a substance.
    pub materials: usize,
    /// Archives placed into a substance's folder (linked, moved or copied), and their bytes.
    pub archives_moved: usize,
    pub bytes_moved: u64,
    /// Shared copies dropped because their substance was there already.
    pub duplicates_removed: usize,
    /// Substances that got a preview image.
    pub previews: usize,
    /// Archives (shared paths) the last run could not convert, and why.
    pub skipped: Vec<(String, String)>,
    /// What was skipped, as it was on disk: tried again only when it changes (or Toybox does).
    #[serde(default)]
    pub skipped_files: Vec<SkippedFile>,
    /// `(material id, substance id)`: a material made from more than one archive, linked to the first; the
    /// archive of this substance left its `origin.files`.
    #[serde(default)]
    pub extra_links: Vec<(String, String)>,
    /// Clean-ups that failed after a commit (an extra name left in `_sources`; harmless). Last run only.
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// An archive a run skipped, with what it was on disk then.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkippedFile {
    pub path: String,
    /// Size and modification time (nanoseconds since 1970) of the file; `None` when it was missing.
    pub bytes: Option<u64>,
    pub modified: Option<u64>,
    /// The Toybox version that skipped it.
    pub app_version: String,
}

/// The journal of one archive in flight.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pending {
    /// Where the archive was (`library/materials/_sources/<sha>/<name>`).
    from: String,
    /// Where it goes (`library/substance/<key>/<name>`).
    to: String,
    /// Files this step writes besides the archive (the preview).
    created: Vec<String>,
    substance: String,
    storage_path: String,
}

/// Why a group was not converted.
enum Failure {
    /// This group only: reported, remembered, the run goes on.
    Skip(String),
    /// Something that must be settled first (a journal that could not be finished): the run stops.
    Abort(AppError),
}

impl From<serde_json::Error> for Failure {
    fn from(error: serde_json::Error) -> Failure {
        Failure::Skip(error.to_string())
    }
}

impl From<AppError> for Failure {
    fn from(error: AppError) -> Failure {
        Failure::Skip(error.to_string())
    }
}

#[cfg(test)]
pub(crate) mod hooks {
    //! Fault injection for the tests (per thread: a test's run happens on its own thread).
    use std::cell::Cell;
    thread_local! {
        /// The next this-many `recover` calls that find a journal fail before they change anything.
        pub static FAIL_RECOVER: Cell<u32> = const { Cell::new(0) };
        /// The next this-many removals of a `_sources` name after a commit fail.
        pub static FAIL_CLEANUP: Cell<u32> = const { Cell::new(0) };
        /// Behave as on a file system without hard links.
        pub static NO_LINKS: Cell<bool> = const { Cell::new(false) };
        /// Full scans of the entries.
        pub static SCANS: Cell<u32> = const { Cell::new(0) };
        /// Archives hashed.
        pub static HASHES: Cell<u32> = const { Cell::new(0) };
    }
    pub fn take(cell: &'static std::thread::LocalKey<Cell<u32>>) -> bool {
        cell.with(|left| left.get() > 0 && {
            left.set(left.get() - 1);
            true
        })
    }
}

/// The id of the substance of an archive with this sha256.
pub fn substance_id(sha256: &str) -> String {
    format!("uid://substance_{}", &sha256[..32.min(sha256.len())])
}

/// `(sha256, name)` of a shared source that is a Substance archive kept by a material.
pub fn sbsar_source(path: &str) -> Option<(&str, &str)> {
    let (folder, sha, name) = sources::parse_shared(path)?;
    (folder == "materials" && name.to_lowercase().ends_with(".sbsar")).then_some((sha, name))
}

fn item_archives(item: &Value) -> impl Iterator<Item = &str> {
    item["origin"]["files"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|file| sbsar_source(file).is_some())
}

fn items_of(entry: &Value) -> impl Iterator<Item = &Value> {
    std::iter::once(entry).chain(entry["variants"].as_array().into_iter().flatten())
}

/// The shared Substance archives a material entry (or one of its variants) lists: what this step converts.
pub fn archives_of(entry: &Value) -> Vec<String> {
    if entry["kind"].as_str() != Some("material") {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for item in items_of(entry) {
        for file in item_archives(item) {
            if !out.iter().any(|known| known == file) {
                out.push(file.to_string());
            }
        }
    }
    out
}

pub fn is_done(store: &Store) -> bool {
    store.meta(META_DONE).as_deref() == Some("done")
}

pub fn report(store: &Store) -> Option<SubstanceMigration> {
    store.meta(META_REPORT).and_then(|text| serde_json::from_str(&text).ok())
}

/// What a full read of the entries found.
#[derive(Default)]
struct Scan {
    /// Every archive to convert and the material items that list it, in id order of the items.
    groups: BTreeMap<String, Vec<String>>,
    /// Every shared Substance archive any entry (of any kind) lists, and the ids of those entries.
    listers: HashMap<String, BTreeSet<String>>,
}

fn scan(store: &Store) -> AppResult<Scan> {
    #[cfg(test)]
    hooks::SCANS.with(|count| count.set(count.get() + 1));
    let mut found = Scan::default();
    let mut after = String::new();
    loop {
        let (entries, next) = store.entries_page_with_origin(&after, PAGE)?;
        for entry in &entries {
            let Some(id) = entry["id"].as_str() else { continue };
            for item in items_of(entry) {
                for file in item_archives(item) {
                    found.listers.entry(file.to_string()).or_default().insert(id.to_string());
                }
            }
            for archive in archives_of(entry) {
                found.groups.entry(archive).or_default().push(id.to_string());
            }
        }
        match next {
            Some(next) => after = next,
            None => break,
        }
    }
    Ok(found)
}

/// Size and modification time of a file; `(None, None)` when it is not there.
fn file_state(path: &Path) -> (Option<u64>, Option<u64>) {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => (Some(meta.len()), meta.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map(|since| since.as_nanos() as u64)),
        _ => (None, None),
    }
}

/// The skip remembered for `path`, when the file is still as it was then and this Toybox skipped it.
fn remembered<'a>(root: &Path, path: &str, skipped: &'a [SkippedFile]) -> Option<&'a SkippedFile> {
    let known = skipped.iter().find(|known| known.path == path && known.app_version == APP_VERSION)?;
    (file_state(&abs(root, path)) == (known.bytes, known.modified)).then_some(known)
}

/// Whether an open (or a write: `ready_for_write` runs the migrations) has work to do: an interrupted
/// archive, or materials that list an archive that was not tried in its current state.
///
/// Cost of the common path, a library with nothing to do: `meta` reads (the manifest revision,
/// `substanceAssetsChecked`, equal until something is written, and the report), plus a `stat` of each
/// remembered skipped archive (none on a healthy library). After a write: one range of the
/// file-reference index (`asset_files`, primary key), which yields only the shared `.sbsar` paths still
/// listed (none on a converted library; the remembered skips otherwise) and a `stat` of each. Never a
/// read of every entry and never a hash. When the derived rows are not current the check waits for the
/// rescan the next write starts (it does not read the entries instead).
pub fn pending(root: &Path, cache: &Path) -> AppResult<bool> {
    if root.join(PENDING).exists() {
        return Ok(true);
    }
    let store = Store::local(root, cache);
    if !is_done(&store) {
        return Ok(true);
    }
    let revision = crate::infra::store::manifest_revision(root)?.to_string();
    let skipped = report(&store).map(|report| report.skipped_files).unwrap_or_default();
    if store.meta(META_CHECKED).as_deref() == Some(revision.as_str()) {
        // Nothing was written since the last look: only a skipped archive that changed on disk (or a new
        // Toybox) is work.
        return Ok(skipped.iter().any(|known| remembered(root, &known.path, std::slice::from_ref(known)).is_none()));
    }
    if !(store.catalog_ready() && store.catalog_exists()) {
        return Ok(false);
    }
    let archives = store.kept_sources_like(&format!("library/{}/{}/", "materials", sources::SOURCES_DIR), ".sbsar")?;
    let left = archives.iter().any(|path| sbsar_source(path).is_some() && remembered(root, path, &skipped).is_none());
    if !left {
        store.set_meta(META_CHECKED, &revision)?;
    }
    Ok(left)
}

impl SubstanceMigration {
    fn skip(&mut self, root: &Path, path: &str, reason: String) {
        let (bytes, modified) = file_state(&abs(root, path));
        self.skipped.retain(|(known, _)| known != path);
        self.skipped_files.retain(|known| known.path != path);
        self.skipped.push((path.to_string(), reason));
        self.skipped_files.push(SkippedFile { path: path.to_string(), bytes, modified, app_version: APP_VERSION.into() });
    }

    fn add(&mut self, run: &SubstanceMigration) {
        self.substances += run.substances;
        self.materials += run.materials;
        self.archives_moved += run.archives_moved;
        self.bytes_moved += run.bytes_moved;
        self.duplicates_removed += run.duplicates_removed;
        self.previews += run.previews;
        for link in &run.extra_links {
            if !self.extra_links.contains(link) {
                self.extra_links.push(link.clone());
            }
        }
    }
}

/// Convert the library at `root` (cache path `cache`). Resumable and idempotent. An error means the run
/// stopped at something the next open must settle first (a journal that could not be finished); what it
/// did until then is recorded.
pub fn migrate(root: &Path, cache: &Path) -> AppResult<SubstanceMigration> {
    let store = Store::local(root, cache);
    let previous = report(&store).unwrap_or_default();
    let mut run = SubstanceMigration::default();
    let mut ran = false;
    let outcome = storage::exclusive(root, cache, |canonical| {
        // `exclusive` finished any storage publication journal before this (or failed).
        ran = true;
        let found = scan(&store)?;
        let listed_at_all = |path: &str| -> AppResult<bool> { Ok(found.listers.contains_key(path)) };
        recover(canonical, cache, &store, &listed_at_all, &mut run)?;
        let todo: Vec<(&String, &Vec<String>)> = found.groups.iter().collect();
        let total = todo.len();
        for (index, (path, ids)) in todo.into_iter().enumerate() {
            if let Some(known) = remembered(canonical, path, &previous.skipped_files) {
                // Tried in this state already: not read again.
                let reason = previous.skipped.iter().find(|(known_path, _)| known_path == path).map(|(_, reason)| reason.clone()).unwrap_or_default();
                run.skipped.push((path.clone(), reason));
                run.skipped_files.push(known.clone());
                continue;
            }
            let (bytes, _) = file_state(&abs(canonical, path));
            eprintln!("toybox: Substance archive {}/{total}: {} ({} MB)", index + 1, sbsar_source(path).map(|(_, name)| name).unwrap_or(path), bytes.unwrap_or(0) / (1024 * 1024));
            // Listed by something this group does not rewrite.
            let elsewhere = |archive: &str| -> AppResult<bool> { Ok(found.listers.get(archive).is_some_and(|all| all.iter().any(|id| !ids.contains(id)))) };
            match convert(canonical, cache, &store, path, ids, &mut run, &elsewhere) {
                Ok(()) => {}
                Err(Failure::Skip(reason)) => run.skip(canonical, path, reason),
                Err(Failure::Abort(error)) => return Err(error),
            }
        }
        Ok(())
    });
    let mut total = previous.clone();
    total.add(&run);
    total.warnings = run.warnings.clone();
    match outcome {
        Ok(()) => {
            // What is still not converted is what this run could not do; earlier failures were tried again
            // or are still remembered.
            total.skipped = run.skipped;
            total.skipped_files = run.skipped_files;
            store.set_meta(META_REPORT, &serde_json::to_string(&total)?)?;
            store.set_meta(META_DONE, "done")?;
            store.set_meta(META_CHECKED, &crate::infra::store::manifest_revision(root)?.to_string())?;
            Ok(total)
        }
        Err(error) => {
            if ran {
                for (path, reason) in run.skipped {
                    total.skipped.retain(|(known, _)| *known != path);
                    total.skipped.push((path, reason));
                }
                for file in run.skipped_files {
                    total.skipped_files.retain(|known| known.path != file.path);
                    total.skipped_files.push(file);
                }
                let _ = store.set_meta(META_REPORT, &serde_json::to_string(&total)?);
            }
            Err(error)
        }
    }
}

/// Convert, inside the storage lock an import holds, the archive `archive` that the items `ids` (and
/// perhaps others) list, before the import replaces them with materials derived from its substance:
/// otherwise the import would remove the archive as a source nothing lists any more. Recorded in the
/// step's report. Fails (and the import with it) when the archive cannot be converted.
pub(crate) fn convert_before_replacement(canonical: &Path, cache: &Path, store: &Store, archive: &str, ids: &[String]) -> AppResult<()> {
    let mut run = SubstanceMigration::default();
    // No scan here: the `_sources` name is removed only when the current derived rows say that nothing
    // lists it; otherwise it stays (the same file as the substance's, or a verified copy).
    let elsewhere = |path: &str| -> AppResult<bool> { Ok(!store.catalog_ready() || store.files_in_use(&[path.to_string()])?.contains(path)) };
    let result = match convert(canonical, cache, store, archive, ids, &mut run, &elsewhere) {
        Ok(()) => Ok(()),
        Err(Failure::Skip(reason)) => Err(AppError::msg(format!("The Substance archive {archive} could not be made an asset before it is replaced: {reason}"))),
        Err(Failure::Abort(error)) => Err(error),
    };
    if run != SubstanceMigration::default() {
        let mut total = report(store).unwrap_or_default();
        total.add(&run);
        total.warnings = run.warnings;
        store.set_meta(META_REPORT, &serde_json::to_string(&total)?)?;
    }
    result
}

fn abs(root: &Path, relative: &str) -> PathBuf {
    paths::abs_under_root(&root.to_string_lossy(), relative)
}

fn sha256_file(path: &Path) -> AppResult<String> {
    use std::io::Read;
    #[cfg(test)]
    hooks::HASHES.with(|count| count.set(count.get() + 1));
    let mut file = std::fs::File::open(path).map_err(|e| AppError::file("Read", path, e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Remove a `_sources` name of an archive whose substance holds it (best effort: a failure is a warning).
fn drop_shared_name(file: &Path, run: &mut SubstanceMigration) {
    #[cfg(test)]
    if hooks::take(&hooks::FAIL_CLEANUP) {
        run.warnings.push(format!("{}: could not be removed (injected); it is an extra name of the substance's archive", file.display()));
        return;
    }
    match fsio::remove_file_if_exists(file) {
        Ok(()) => retention::remove_set_folder_if_empty(file),
        Err(error) => {
            let warning = format!("{}: could not be removed ({error}); it is an extra name of the substance's archive", file.display());
            eprintln!("toybox: {warning}");
            run.warnings.push(warning);
        }
    }
}

/// Settle an archive that was in flight when a run stopped (see the module documentation). Returns
/// whether there was one and, if so, whether its commit had landed. `elsewhere` says whether an entry
/// lists a path (other than the items the journal's commit rewrote). An error leaves the journal: the
/// run must stop.
fn recover(root: &Path, cache: &Path, store: &Store, elsewhere: &dyn Fn(&str) -> AppResult<bool>, run: &mut SubstanceMigration) -> AppResult<Option<bool>> {
    let path = root.join(PENDING);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(AppError::file("Read", &path, error)),
    };
    // Never decide from the database while a publication can still be replayed: finish it first.
    storage::settle(root, cache)?;
    #[cfg(test)]
    if hooks::take(&hooks::FAIL_RECOVER) {
        return Err(AppError::msg("The archive could not be moved back (injected)"));
    }
    let pending: Pending = serde_json::from_slice(&bytes)?;
    for relative in [&pending.from, &pending.to].into_iter().chain(&pending.created) {
        if !relative.starts_with("library/") || relative.split('/').any(|part| part.is_empty() || part == "." || part == "..") || relative.contains(['\\', ':']) {
            return Err(AppError::msg(format!("Invalid substance migration journal: {relative}")));
        }
    }
    let committed = store.load_entries(std::slice::from_ref(&pending.substance))?.first().and_then(|entry| entry["storagePath"].as_str().map(String::from)) == Some(pending.storage_path.clone());
    let (from, to) = (abs(root, &pending.from), abs(root, &pending.to));
    if committed {
        // The substance names `to`. `from` is an extra name of the same file (or a verified copy) once
        // nothing lists it.
        if !to.is_file() {
            if from.is_file() {
                // Not a state this step leaves; put the archive where the substance names it.
                place(&from, &to, false, None)?;
            } else {
                run.warnings.push(format!("{}: the archive of {} is missing", pending.to, pending.substance));
            }
        }
        if from.is_file() && to.is_file() && !elsewhere(&pending.from).unwrap_or(true) {
            drop_shared_name(&from, run);
        }
    } else {
        // Nothing names the new place: what this step added goes; a renamed archive goes back.
        if to.is_file() {
            if from.is_file() {
                fsio::remove_file_if_exists(&to)?;
            } else {
                if let Some(parent) = from.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| AppError::file("Create directory", parent, e))?;
                }
                if std::fs::rename(&to, &from).is_err() {
                    // The archive must be where the materials name it: a link or a copy will do.
                    place(&to, &from, false, None).map_err(|error| AppError::msg(format!("The archive could not be put back to {}: {error}", pending.from)))?;
                    fsio::remove_file_if_exists(&to)?;
                }
            }
        }
        fsio::remove_file_if_exists(&to.with_extension("copying"))?;
        for created in &pending.created {
            fsio::remove_file_if_exists(&abs(root, created))?;
        }
        retention::remove_set_folder_if_empty(&to);
    }
    fsio::remove_file_if_exists(&path)?;
    Ok(Some(committed))
}

/// How an item was linked to an archive's substance.
#[derive(PartialEq)]
enum Linked {
    No,
    /// `derivedFrom` names the substance.
    Yes,
    /// The archive left `origin.files`, but the item is linked to an archive it lists first.
    Extra,
}

/// Link one item (an entry or a variant, as a manifest entry) to its substance.
fn link_entry(item: &mut Value, archive: &str, substance: &str) -> Linked {
    let first = item_archives(item).next().map(String::from);
    let Some(files) = item.get_mut("origin").and_then(|origin| origin.get_mut("files")).and_then(Value::as_array_mut) else { return Linked::No };
    let before = files.len();
    files.retain(|file| file.as_str() != Some(archive));
    if files.len() == before {
        return Linked::No;
    }
    let empty = files.is_empty();
    let origin = item["origin"].as_object_mut().expect("an origin with files");
    if empty {
        origin.remove("files");
    }
    let linked = if first.as_deref() == Some(archive) {
        origin.insert("derivedFrom".into(), json!(substance));
        Linked::Yes
    } else {
        Linked::Extra
    };
    if item.get("format").is_none_or(Value::is_null) {
        item["format"] = json!("sbsar");
    }
    linked
}

/// [`link_entry`] for the catalog asset of an item (and its variants).
fn link_asset(asset: &mut crate::domain::catalog_model::Asset, archive: &str, substance: &str) {
    if let Some(origin) = asset.origin.as_mut() {
        let first = origin.files.iter().find(|file| sbsar_source(file).is_some()).cloned();
        let before = origin.files.len();
        origin.files.retain(|file| file != archive);
        if origin.files.len() != before {
            if first.as_deref() == Some(archive) {
                origin.derived_from = Some(substance.to_string());
            }
            if asset.format.is_none() {
                asset.format = Some("sbsar".into());
            }
        }
    }
    for variant in &mut asset.variants {
        link_asset(variant, archive, substance);
    }
}

/// A square PNG of at most [`PREVIEW_SIZE`] pixels from a material's albedo map; `None` when it cannot be read.
fn preview_of(root: &Path, entry: &Value) -> Option<Vec<u8>> {
    let folder = entry["storagePath"].as_str()?;
    let maps = entry["maps"].as_array()?;
    let map = maps.iter().find(|map| map["role"].as_str() == Some("albedo")).or_else(|| maps.first())?;
    let file = abs(root, &format!("{folder}/{}", map["file"].as_str()?));
    let image = image::ImageReader::open(&file).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let side = image.width().min(image.height());
    if side == 0 {
        return None;
    }
    let square = image.crop_imm((image.width() - side) / 2, (image.height() - side) / 2, side, side);
    let size = side.min(PREVIEW_SIZE);
    let small = square.resize_exact(size, size, image::imageops::FilterType::Triangle).to_rgba8();
    let mut out = std::io::Cursor::new(Vec::new());
    small.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// A display name that is a valid plugin asset name.
fn plain_name(name: &str) -> String {
    let cleaned: String = name.trim().chars().map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." { "Substance".into() } else { cleaned }
}

/// How an archive got its name in the substance's folder.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Placed {
    /// A hard link: the same file under a second name.
    Link,
    /// Renamed: no longer at its `_sources` path until the commit or the rollback.
    Rename,
    /// A copy, verified against `sha256` already.
    Copy,
}

/// Give the archive at `from` the name `to`: a hard link; where that fails a rename (only when
/// `may_rename`); where that fails a copy, verified against `sha256` when given (one read of the copy).
fn place(from: &Path, to: &Path, may_rename: bool, sha256: Option<&str>) -> AppResult<Placed> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::file("Create directory", parent, e))?;
    }
    if to.exists() {
        return Err(AppError::msg(format!("{} is there already", to.display())));
    }
    #[cfg(test)]
    let links = !hooks::NO_LINKS.with(|cell| cell.get());
    #[cfg(not(test))]
    let links = true;
    if links && std::fs::hard_link(from, to).is_ok() {
        return Ok(Placed::Link);
    }
    if may_rename && std::fs::rename(from, to).is_ok() {
        return Ok(Placed::Rename);
    }
    let copying = to.with_extension("copying");
    std::fs::copy(from, &copying).map_err(|e| AppError::file("Copy", from, e))?;
    if let Some(sha256) = sha256 {
        let copied = sha256_file(&copying);
        if copied.as_deref().ok() != Some(sha256) {
            let _ = std::fs::remove_file(&copying);
            return Err(AppError::msg(format!("The archive does not match its name (sha256 {})", copied.unwrap_or_default())));
        }
    }
    std::fs::rename(&copying, to).map_err(|e| AppError::file("Move", &copying, e))?;
    Ok(Placed::Copy)
}

/// Publish the substance (when given) and the linked materials in one journaled commit.
#[allow(clippy::too_many_arguments)]
fn publish(canonical: &Path, cache: &Path, substance: Option<(Value, Vec<crate::domain::catalog_model::Asset>)>, materials: Vec<Value>, mut assets: Vec<crate::domain::catalog_model::Asset>, before: BTreeMap<String, Value>, hashes: Vec<(String, String)>, expect: i64) -> AppResult<()> {
    let substance_id = substance.as_ref().and_then(|(entry, _)| entry["id"].as_str().map(String::from));
    let (substance_entry, mut substance_assets) = match substance {
        Some((entry, assets)) => (Some(entry), assets),
        None => (None, Vec::new()),
    };
    let entries: Vec<Value> = substance_entry.into_iter().chain(materials).collect();
    let mut plan = ItemPlan::new(canonical, entries)?;
    plan.place()?;
    let mut puts = Vec::new();
    for entry in plan.entries() {
        let id = entry["id"].as_str().unwrap_or("").to_string();
        let mine: Vec<_> = if Some(&id) == substance_id.as_ref() {
            std::mem::take(&mut substance_assets)
        } else {
            let (mine, rest): (Vec<_>, Vec<_>) = assets.into_iter().partition(|asset| asset.id == id);
            assets = rest;
            mine
        };
        puts.push(ItemPut { id, entry: Some(entry.clone()), assets: mine, rigs: Vec::new() });
    }
    plan.commit(cache, before, Delta { puts, hashes, expect_manifest_revision: Some(expect), ..Default::default() })
}

/// A failed publication: if its journal is left, it is finished now (it may land after all); if that
/// cannot be done the run stops with the journal in place.
fn settle_publication(canonical: &Path, cache: &Path) -> Result<(), Failure> {
    if storage::publication_pending(canonical) {
        storage::settle(canonical, cache).map_err(Failure::Abort)?;
    }
    Ok(())
}

/// Convert one archive and the items that list it. `elsewhere` says whether an entry other than the
/// items `ids` lists a path.
fn convert(canonical: &Path, cache: &Path, store: &Store, archive: &str, ids: &[String], run: &mut SubstanceMigration, elsewhere: &dyn Fn(&str) -> AppResult<bool>) -> Result<(), Failure> {
    let (sha, name) = sbsar_source(archive).ok_or_else(|| Failure::Skip("not a shared Substance archive".into()))?;
    let (sha, name) = (sha.to_string(), name.to_string());
    let substance = substance_id(&sha);
    let entries: Vec<Value> = store.load_entries(ids)?.into_iter().filter(|entry| archives_of(entry).iter().any(|path| path == archive)).collect();
    if entries.is_empty() {
        return Ok(());
    }
    let expect = crate::infra::store::manifest_revision(canonical)?;
    let before: BTreeMap<String, Value> = entries.iter().filter_map(|entry| Some((entry["id"].as_str()?.to_string(), entry.clone()))).collect();
    let mut linked = entries.clone();
    let mut extra = Vec::new();
    for entry in &mut linked {
        let id = entry["id"].as_str().unwrap_or("").to_string();
        let mut outcomes = vec![link_entry(entry, archive, &substance)];
        for variant in entry.get_mut("variants").and_then(Value::as_array_mut).into_iter().flatten() {
            outcomes.push(link_entry(variant, archive, &substance));
        }
        if outcomes.contains(&Linked::Extra) {
            extra.push((id, substance.clone()));
        }
    }
    let item_ids: Vec<String> = before.keys().cloned().collect();
    let mut assets = store.load_top_assets(&item_ids)?;
    for asset in &mut assets {
        link_asset(asset, archive, &substance);
    }
    let from = abs(canonical, archive);
    let materials = linked.len();

    if let Some(existing) = store.load_entries(std::slice::from_ref(&substance))?.into_iter().next() {
        // The substance is there: link, then drop the duplicate copy once nothing lists it.
        if existing["meta"]["sha256"].as_str().is_some_and(|known| known != sha) {
            return Err(Failure::Skip(format!("{substance} holds another archive")));
        }
        let own = existing["storagePath"].as_str().zip(existing["meta"]["sbsar"].as_str().or_else(|| existing["files"][0].as_str())).map(|(folder, file)| abs(canonical, &format!("{folder}/{file}")));
        let Some(own) = own.filter(|own| own.is_file()) else {
            return Err(Failure::Skip(format!("{substance} is in the library but its archive is not; the materials keep theirs")));
        };
        if let Err(error) = publish(canonical, cache, None, linked, assets, before, Vec::new(), expect) {
            settle_publication(canonical, cache)?;
            if !entries_linked(store, &item_ids, archive)? {
                return Err(Failure::Skip(error.to_string()));
            }
        }
        run.materials += materials;
        run.extra_links.extend(extra);
        // Past the commit: a failure to tell keeps the copy (never an error that would mark the group skipped).
        if from.is_file() && !elsewhere(archive).unwrap_or(true) {
            // Same content (a hard link or a copy) is checked before a name goes: it may be the only copy
            // of something else.
            let same = std::fs::metadata(&own).map(|meta| meta.len()).ok() == std::fs::metadata(&from).map(|meta| meta.len()).ok() && sha256_file(&from).ok().as_deref() == Some(sha.as_str());
            if same {
                drop_shared_name(&from, run);
                run.duplicates_removed += 1;
            } else {
                run.skip(canonical, archive, "the shared copy does not match its name; it was kept (nothing lists it)".into());
            }
        }
        return Ok(());
    }

    let bytes = std::fs::metadata(&from).ok().filter(|meta| meta.is_file() && meta.len() > 0).map(|meta| meta.len()).ok_or_else(|| Failure::Skip("the archive is missing".into()))?;
    let first = &entries[0];
    // Name: the one material's when only one uses the archive, otherwise the archive's.
    let users: Vec<&Value> = entries.iter().flat_map(items_of).filter(|item| item_archives(item).any(|path| path == archive)).collect();
    let stem = Path::new(&name).file_stem().map(|stem| stem.to_string_lossy().to_string()).unwrap_or_else(|| name.clone());
    let display = match users.as_slice() {
        [only] => only["name"].as_str().map(String::from).unwrap_or(stem),
        _ => stem,
    };
    let category = first["category"].as_str().filter(|category| !category.trim().is_empty()).unwrap_or(TYPE_ID);
    let plugin = first["origin"]["plugin"].as_str().filter(|plugin| !plugin.trim().is_empty()).unwrap_or(DEFAULT_PLUGIN);
    let preview = preview_of(canonical, first);
    let mut files = vec![json!(name)];
    let mut digests = vec![sha.clone()];
    if let Some(bytes) = &preview {
        files.push(json!(PREVIEW));
        digests.push(format!("{:x}", Sha256::digest(bytes)));
    }
    let mut entry = json!({
        "kind": "plugin", "type": TYPE_ID, "id": substance,
        "pack": first["pack"].as_str().unwrap_or("imported"), "category": category,
        "name": plain_name(&display), "format": "sbsar", "files": files,
        "meta": { "sbsar": name, "sha256": sha, "bytes": bytes },
        "origin": { "plugin": plugin },
    });
    for field in ["tags", "provenance"] {
        if let Some(value) = first.get(field).filter(|value| !value.is_null()) {
            entry[field] = value.clone();
        }
    }
    // The folder a commit of this entry with these files would get.
    let storage_path = storage::planned_folder(&entry, &digests)?;
    entry["storagePath"] = json!(storage_path);
    let to_rel = format!("{storage_path}/{name}");
    let preview_rel = format!("{storage_path}/{PREVIEW}");
    let to = abs(canonical, &to_rel);
    let pending = Pending { from: archive.to_string(), to: to_rel.clone(), created: if preview.is_some() { vec![preview_rel.clone()] } else { Vec::new() }, substance: substance.clone(), storage_path };
    // Never over another journal: it is the only record of an archive in flight.
    let journal = canonical.join(PENDING);
    if journal.exists() {
        return Err(Failure::Abort(AppError::msg("An earlier Substance archive conversion is not settled; the next open settles it")));
    }
    // A rename takes the archive away from its `_sources` path until the commit: only when nothing but
    // this group lists it.
    let may_rename = !elsewhere(archive)?;
    // The journal first: from here on an interruption is settled by `recover`.
    fsio::write_bytes_atomic(&journal, &serde_json::to_vec_pretty(&pending)?)?;
    let result = (|| -> AppResult<()> {
        if let Some(bytes) = &preview {
            fsio::write_bytes_atomic(&abs(canonical, &preview_rel), bytes)?;
        }
        let placed = place(&from, &to, may_rename, Some(&sha))?;
        if placed != Placed::Copy {
            // The one read of the archive: its content must be what its name says.
            let actual = sha256_file(&to)?;
            if actual != sha {
                return Err(AppError::msg(format!("The archive does not match its name (sha256 {actual}); it was left as it was")));
            }
        }
        let parsed = seed::parse_entry(&entry)?;
        let (catalog, _) = crate::ops::catalog_build::build_catalog_with(&canonical.to_string_lossy(), vec![parsed], store)?;
        let mut hasher = crate::ops::content_hash::ContentHasher::new(canonical);
        hasher.assume_digest(&to_rel, &sha);
        let hashes: Vec<(String, String)> = catalog.assets.iter().filter_map(|asset| Some((crate::ops::content_hash::storage_key(asset)?, hasher.hash_asset(asset).ok()?))).collect();
        publish(canonical, cache, Some((entry.clone(), catalog.assets)), linked, assets, before, hashes, expect)
    })();
    // Before anything is decided from the database, a publication journal left by a failed commit is
    // finished (the commit may land after all); if it cannot be, the run stops here.
    if result.is_err() {
        settle_publication(canonical, cache)?;
    }
    // Settles the journal exactly as a later open would: by whether the commit is there.
    let committed = recover(canonical, cache, store, elsewhere, run).map_err(Failure::Abort)?.unwrap_or(false);
    if !committed {
        return Err(Failure::Skip(result.err().map(|error| error.to_string()).unwrap_or_else(|| "the commit did not land".into())));
    }
    run.substances += 1;
    run.materials += materials;
    run.archives_moved += 1;
    run.bytes_moved += bytes;
    run.previews += usize::from(preview.is_some());
    run.extra_links.extend(extra);
    Ok(())
}

/// Whether none of the items `ids` lists `archive` any more (a commit that failed but was finished).
fn entries_linked(store: &Store, ids: &[String], archive: &str) -> AppResult<bool> {
    Ok(!store.load_entries(ids)?.iter().any(|entry| archives_of(entry).iter().any(|path| path == archive)))
}

#[cfg(test)]
pub(crate) fn pending_journal(root: &Path) -> PathBuf {
    root.join(PENDING)
}
