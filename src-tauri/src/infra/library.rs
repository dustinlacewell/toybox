//! The runtime library root: the single live source of truth for "where the
//! asset library lives." It replaces the former compile-time `config::LIBRARY_ROOT`
//! const so the location is the user's choice, persisted in `settings.json` and
//! held in Tauri-managed state.
//!
//! Three responsibilities live here because they are inseparable: validating that
//! a folder is a usable library, persisting the choice, and extending the
//! asset-protocol scope so the webview can actually load files from it (the
//! scope is otherwise a static allowlist that wouldn't cover an arbitrary path).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::config;
use crate::domain::paths;
use crate::error::{AppError, AppResult};
use crate::infra::{fsio, settings};

/// Tauri-managed live root. `None` until the user has chosen a library.
#[derive(Default)]
pub struct LibraryState(pub Mutex<Option<PathBuf>>);

/// Hydrate the live root from persisted settings and, if one is present and
/// valid, extend the asset-protocol scope to it. Called once from `.setup()`.
/// A persisted-but-now-missing root is dropped (left unset) so the app falls
/// back to the picker rather than erroring on a stale path.
pub fn hydrate(app: &AppHandle) -> AppResult<()> {
    let Some(root) = settings::load(app)?.library_root.map(PathBuf::from) else {
        return Ok(());
    };
    if validate(&root).is_err() {
        return Ok(());
    }
    extend_scope(app, &root);
    store(app, Some(root));
    Ok(())
}

/// The current root, or `AppError::NoLibrary` if none is configured. Every file
/// read/write keyed to the library funnels through this, so an unconfigured app
/// fails with a distinguishable sentinel rather than a raw IO error.
pub fn resolve(app: &AppHandle) -> AppResult<PathBuf> {
    current(app).ok_or(AppError::NoLibrary)
}

/// The current root without erroring — for the picker gate (`get_library_root`).
pub fn current(app: &AppHandle) -> Option<PathBuf> {
    app.state::<LibraryState>().0.lock().unwrap().clone()
}

/// Adopt a user-chosen folder: validate it is a Toybox-style library, persist
/// the choice, extend the asset-protocol scope, and update the live root.
pub fn set(app: &AppHandle, root: &Path) -> AppResult<()> {
    validate(root)?;
    let root = root.to_path_buf();
    // Preserve any other settings (e.g. the converter path) — only the root changes.
    let mut settings = settings::load(app)?;
    settings.library_root = Some(root.to_string_lossy().to_string());
    settings::save(app, &settings)?;
    extend_scope(app, &root);
    store(app, Some(root));
    Ok(())
}

/// Establish a new, empty library in `root`, then adopt it. An already-valid
/// library is adopted as-is (idempotent); otherwise the folder must be empty, so
/// scaffolding can never clobber unrelated files. The skeleton is the minimum a
/// scan accepts: an empty seed catalog and the `library/` packs directory that
/// importers populate.
pub fn create(app: &AppHandle, root: &Path) -> AppResult<()> {
    if validate(root).is_err() {
        require_empty_dir(root)?;
        scaffold(root)?;
    }
    set(app, root)
}

/// A valid library is a Toybox-curated tree: it must carry the native catalog
/// seed at `_library_config/catalog.json`. Anything else is rejected with a
/// message the picker shows verbatim.
fn validate(root: &Path) -> AppResult<()> {
    if !root.is_dir() {
        return Err(AppError::msg(format!(
            "not a folder: {}",
            root.to_string_lossy()
        )));
    }
    let seed = paths::abs_under_root(&root.to_string_lossy(), config::SEED_REL);
    if !fsio::exists(&seed) {
        return Err(AppError::msg(
            "not a Toybox library: missing _library_config/catalog.json".to_string(),
        ));
    }
    Ok(())
}

/// Refuse to scaffold into a folder that already holds files. Creating into an
/// empty (or not-yet-existing) folder is fine; anything else risks surprising
/// the user, so it's an error they can act on.
fn require_empty_dir(root: &Path) -> AppResult<()> {
    if !root.exists() {
        return Ok(());
    }
    if !root.is_dir() {
        return Err(AppError::msg(format!(
            "not a folder: {}",
            root.to_string_lossy()
        )));
    }
    let mut entries = std::fs::read_dir(root)?;
    if entries.next().is_some() {
        return Err(AppError::msg(
            "folder is not empty — choose an empty folder for a new library".to_string(),
        ));
    }
    Ok(())
}

/// Lay down the minimum a library needs: an empty seed catalog at
/// `_library_config/catalog.json` (the parent dir is created by the write) and
/// the `library/` directory importers add packs under.
fn scaffold(root: &Path) -> AppResult<()> {
    let seed = paths::abs_under_root(&root.to_string_lossy(), config::SEED_REL);
    fsio::write_text(&seed, EMPTY_SEED)?;
    std::fs::create_dir_all(root.join("library"))?;
    Ok(())
}

/// The native seed source for an empty library. `schemaVersion: 1` is the
/// *source* schema the seed parser reads (distinct from the cache
/// `config::SCHEMA_VERSION`); `assets: []` is what an empty library declares.
const EMPTY_SEED: &str = "{\n  \"schemaVersion\": 1,\n  \"assets\": []\n}\n";

/// Allow the webview's asset protocol to serve files under `root`. Without this
/// the catalog scans fine but every gltf/texture/thumbnail 404s in the viewer.
fn extend_scope(app: &AppHandle, root: &Path) {
    // Best-effort: a scope error here shouldn't abort adopting the library; the
    // viewer would degrade, but scanning/exports still work.
    let _ = app.asset_protocol_scope().allow_directory(root, true);
}

fn store(app: &AppHandle, root: Option<PathBuf>) {
    *app.state::<LibraryState>().0.lock().unwrap() = root;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::seed::parse_seed_entries;

    /// A unique temp dir for one test, removed on drop so a failed run doesn't
    /// leak. Names are derived from the test, not a clock/RNG, so they're stable.
    struct TmpDir(PathBuf);
    impl TmpDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("toybox_lib_test_{tag}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TmpDir(dir)
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn scaffold_produces_a_library_that_validates_and_scans_empty() {
        let tmp = TmpDir::new("scaffold");
        scaffold(&tmp.0).expect("scaffold");

        // The skeleton is a valid library, with the packs dir present.
        validate(&tmp.0).expect("scaffolded dir should validate");
        assert!(tmp.0.join("library").is_dir(), "library/ dir created");

        // The seed the scan reads parses as an empty asset list — no crash, no
        // phantom assets.
        let seed = paths::abs_under_root(&tmp.0.to_string_lossy(), config::SEED_REL);
        let doc: serde_json::Value =
            serde_json::from_str(&fsio::read_text(&seed).unwrap()).unwrap();
        assert!(parse_seed_entries(&doc).unwrap().is_empty(), "empty seed");
    }

    #[test]
    fn require_empty_dir_accepts_empty_or_absent_and_rejects_occupied() {
        let tmp = TmpDir::new("empty");
        require_empty_dir(&tmp.0).expect("fresh empty dir is allowed");
        require_empty_dir(&tmp.0.join("does-not-exist")).expect("absent dir is allowed");

        fsio::write_text(&tmp.0.join("stray.txt"), "x").unwrap();
        assert!(require_empty_dir(&tmp.0).is_err(), "occupied dir is rejected");
    }
}
