//! Catalog cache read/write for one library. These work on a `LibraryHandle`
//! (explicit paths), so the desktop commands and a server share them. The cache
//! lives in the library's databases (`infra::store`).

use crate::config;
use crate::domain::catalog_model::{Asset, Catalog, ThumbState};
use crate::error::AppResult;
use crate::infra::fsio;
use crate::infra::registry::{self, LibraryHandle};
use crate::infra::store::Store;

/// Hand back the cache only if it was built under the current schema. A cache
/// from an older schema may carry stale *derived* fields (e.g. a v1 `clipCount`
/// predating the animation channel filter), so we reject it as `None` — the
/// caller then rescans, which rebuilds those fields while a scan reads the same
/// stale rows internally (`load_catalog_inner`) to preserve user metadata
/// (favorites/tags survive a schema bump).
pub fn load_current_catalog(handle: &LibraryHandle) -> AppResult<Option<Catalog>> {
    let mut catalog = load_catalog_inner(handle)?.filter(|c| c.schema_version == config::SCHEMA_VERSION);
    if let Some(catalog) = &mut catalog {
        // Content fingerprints come from the side cache, never from the stored catalog.
        super::content_hash::fill_from_cache(handle, catalog);
    }
    Ok(catalog)
}

/// Write the user-edited catalog of one library. The root and the rig registry
/// are the library's own facts, so they are taken from the handle and the cache
/// on disk rather than from the caller's (possibly merged) copy.
pub fn save_user_catalog(handle: &LibraryHandle, catalog: Catalog) -> AppResult<()> {
    let mut catalog = catalog;
    let on_disk = Store::for_handle(handle).load_catalog().ok().flatten();
    if let Some(disk) = &on_disk { catalog.rigs = disk.rigs.clone(); }
    // Library tags belong to the manifest, not to a user edit of the catalog: a
    // copy that was read before they changed must not put the old ones back.
    if let Some(disk) = &on_disk { keep_library_tags(&mut catalog.assets, &disk.assets); }
    save_catalog_inner(handle, &catalog)
}

fn keep_library_tags(assets: &mut [Asset], disk: &[Asset]) {
    fn collect<'a>(items: &'a [Asset], known: &mut std::collections::HashMap<&'a str, &'a [String]>) {
        for item in items {
            known.insert(item.id.as_str(), item.tags.as_slice());
            collect(&item.variants, known);
        }
    }
    fn apply(assets: &mut [Asset], known: &std::collections::HashMap<&str, &[String]>) {
        for asset in assets {
            if let Some(tags) = known.get(asset.id.as_str()) { asset.tags = tags.to_vec(); }
            apply(&mut asset.variants, known);
        }
    }
    let mut known = std::collections::HashMap::new();
    collect(disk, &mut known);
    apply(assets, &known);
}

/// Make a local library ready for an incremental write: finish any migration or
/// interrupted publication, and build the catalog if there is none that is current
/// (an explicit rescan, which only a library that never had one pays for). After
/// this the store takes `Store::apply` without any whole-catalog work.
pub fn ready_for_write(handle: &LibraryHandle) -> AppResult<()> {
    if handle.is_remote() {
        return Ok(());
    }
    crate::infra::migrations::run(&handle.root, &handle.cache_path)?;
    if !Store::for_handle(handle).catalog_ready() {
        super::scan::scan_library_catalog(handle)?;
    }
    Ok(())
}

/// What opening a library does before anything is read from it: a local library
/// finishes its migrations and any interrupted publication; a remote library's
/// older cache file becomes its replica.
pub fn open_library(handle: &LibraryHandle) -> AppResult<()> {
    // A remote library's root is only a file cache: the server owns the manifest, so there is nothing to migrate here.
    if !handle.is_remote() {
        crate::infra::migrations::run(&handle.root, &handle.cache_path)?;
    } else {
        // A remote library cached before the databases existed: its catalog becomes the replica.
        crate::infra::store::adopt::adopt_remote_cache(handle)?;
    }
    Ok(())
}

pub fn load_catalog_inner(handle: &LibraryHandle) -> AppResult<Option<Catalog>> {
    open_library(handle)?;
    let Some(mut catalog) = Store::for_handle(handle).load_catalog()? else { return Ok(None) };
    crate::infra::audio::remember(std::path::Path::new(&catalog.library_root),&catalog.assets);
    refresh_animation_preview_thumbs(handle)?;
    reconcile_thumb_states(handle, &mut catalog)?;
    Ok(Some(catalog))
}

/// Once per library, old animation pictures are refreshed without discarding
/// them first. The done-marker is a file in the library's own app-data folder
/// (it used to be one global setting, which could only cover one library).
pub(crate) fn refresh_animation_preview_thumbs(handle:&LibraryHandle)->AppResult<()> {
    static MIGRATION:std::sync::Mutex<()>=std::sync::Mutex::new(());
    let _guard=MIGRATION.lock().map_err(|_|crate::error::AppError::msg("Preview migration lock was poisoned"))?;
    const VERSION:u32=1;
    let dir=handle.data_dir();
    if registry::marker(&dir,registry::PREVIEW_MARKER)>=VERSION{return Ok(());}
    for id in Store::for_handle(handle).asset_ids_of_kind("animation")? {
        let path=handle.thumb_path(&id);
        // Refresh the old animation picture without discarding it first.
        fsio::write_bytes(&path.with_extension("stale"),&[])?;
    }
    registry::set_marker(&dir,registry::PREVIEW_MARKER,VERSION)
}

/// The thumbnail PNG on disk is the source of truth for "is this thumb ready".
/// We never churn the catalog with per-thumbnail writes; instead, every
/// time the catalog is handed to the frontend we derive `ready`/`missing` from
/// disk. An `error` recorded on disk-less assets is preserved.
fn reconcile_thumb_states(handle: &LibraryHandle, catalog: &mut Catalog) -> AppResult<()> {
    for asset in &mut catalog.assets {
        let exists = handle.has_thumb(&asset.id);
        asset.thumb.state = match (exists, &asset.thumb.state) {
            (true, _) => ThumbState::Ready,
            (false, ThumbState::Error) => ThumbState::Error,
            (false, _) => ThumbState::Missing,
        };
    }
    Ok(())
}

pub fn save_catalog_inner(handle: &LibraryHandle, catalog: &Catalog) -> AppResult<()> {
    // A local library's fingerprints are derived (the hashes table) and the store
    // keeps none with the rows; a remote library's are the server's, kept so the
    // catalog reads the same offline.
    Store::for_handle(handle).save_catalog(catalog).map(|_| ())
}
