//! `merge_seed_entries`: the importer commit. An importer's entries have exactly
//! the shape of a manifest (`_library_config/catalog.json`) entry; the manifest is
//! the single source of truth, so the commit upserts them into it and the cached
//! catalog gains the new assets without losing any other.
//!
//! Order matters: every entry is parsed and its files validated (via
//! `build_catalog`) before anything is written, so a bad import leaves the
//! library untouched. The referenced files must already exist under the library.

use serde_json::Value;
use tauri::AppHandle;

use crate::config;
use crate::domain::catalog_model::Catalog;
use crate::domain::{merge, paths, seed};
use crate::error::AppResult;
use crate::infra::{fsio, library};

use super::catalog::{load_catalog_inner, save_catalog_inner};
use super::catalog_build::build_catalog;
use super::scan::build_catalog_from_seed;

#[tauri::command]
pub async fn merge_seed_entries(app: AppHandle, entries: Vec<Value>) -> AppResult<Catalog> {
    let root = library::resolve(&app)?.to_string_lossy().to_string();

    let parsed = entries.iter().map(seed::parse_entry).collect::<AppResult<Vec<_>>>()?;
    let fresh = build_catalog(&root, parsed)?;

    upsert_into_manifest(&root, entries)?;

    let result = match load_catalog_inner(&app)? {
        Some(prior) => merge::upsert_assets(prior, fresh.assets),
        None => build_catalog_from_seed(&root)?,
    };

    save_catalog_inner(&app, &result)?;
    Ok(result)
}

/// Read the manifest, upsert `entries` into it, and write it back pretty-printed.
fn upsert_into_manifest(root: &str, entries: Vec<Value>) -> AppResult<()> {
    let path = paths::abs_under_root(root, config::SEED_REL);
    let doc = serde_json::from_str(&fsio::read_text(&path)?)?;
    let updated = seed::upsert_entries(doc, entries)?;
    fsio::write_text(&path, &serde_json::to_string_pretty(&updated)?)
}
