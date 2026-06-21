//! `merge_seed_entries`: the importer commit. Turns plugin-supplied seed entries
//! into catalog assets and merges them in, preserving prior user metadata —
//! exactly the pipeline `scan_library` runs, but seeded from a plugin instead of
//! the hardcoded `catalog.json`. This is what finally opens the previously-closed
//! inbound format: any importer plugin can contribute assets by producing entries.
//!
//! The referenced files must already exist under `library/<pack>/<category>/` for
//! the glTF facet read to resolve (same requirement as scan); a missing file
//! surfaces a clear error rather than a partial catalog.

use serde::Deserialize;
use tauri::AppHandle;

use crate::domain::catalog_model::Catalog;
use crate::domain::merge;
use crate::domain::seed::entry_from_parts;
use crate::error::AppResult;

use super::catalog::{load_catalog_inner, save_catalog_inner};
use super::catalog_build::build_catalog;

/// One entry an importer produced. Mirrors the four declared fields of the native
/// `catalog.json` source (`id`, `pack`, `category`, `file`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedEntryInputDto {
    pub id: String,
    pub pack: String,
    pub category: String,
    pub file: String,
}

#[tauri::command]
pub async fn merge_seed_entries(
    app: AppHandle,
    entries: Vec<SeedEntryInputDto>,
) -> AppResult<Catalog> {
    let seed_entries = entries
        .into_iter()
        .map(|e| entry_from_parts(e.id, e.pack, e.category, e.file))
        .collect();
    let fresh = build_catalog(seed_entries)?;

    let result = match load_catalog_inner(&app)? {
        Some(prior) => merge::merge_preserving_user(&prior, fresh),
        None => fresh,
    };

    save_catalog_inner(&app, &result)?;
    Ok(result)
}
