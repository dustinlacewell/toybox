//! Operations on the items of one library (named by its id), routed to the local
//! code in `toybox_core::ops` or, for a remote library, to the server through
//! `toybox_remote`. Every function blocks; a remote library may use the network.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use toybox_core::domain::catalog_model::{Asset, ThumbState};
use toybox_core::domain::export_model::{ExportCopyReq, ExportGlbReq, ExportReport};
use toybox_core::domain::gltf_origin::{Aabb, Align, Axis};
use toybox_core::domain::pack_meta::PackMeta;
use toybox_core::error::{AppError, AppResult};
use toybox_core::infra::store::Store;
use toybox_core::ops::animation_preview::{self, PreviewChoice};
use toybox_core::ops::browse::{summary, LibrarySummary};
use toybox_core::ops::export_plugin as export_ops;
use toybox_core::ops::metadata::{update_entries, MetadataEdit};
use toybox_core::ops::transfer::{OnConflict, TransferMode, TransferOptions, TransferProgress, TransferReport};
use toybox_core::ops::preview_source;
use toybox_remote::transfer::{transfer, Side};

use crate::fingerprint::FingerprintHashes;
use crate::host::Host;

/// What a delete did: the ids that are gone, and where the library stands.
#[derive(Serialize)]
pub struct Deleted {
    pub deleted: Vec<String>,
    pub summary: LibrarySummary,
}

/// What an import commit did.
#[derive(Debug, Clone)]
pub struct MergeReport {
    pub summary: LibrarySummary,
    /// Ids of the items the commit wrote, as the library lists them.
    pub ids: Vec<String>,
}

/// One progress report of a transfer, with the libraries it is between.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgressEvent {
    pub from_library: String,
    pub to_library: String,
    #[serde(flatten)]
    pub progress: TransferProgress,
}

impl Host {
    // --- scan, delete, import, metadata, transfer ---------------------------

    /// The explicit rescan: rebuild a local library's catalog from its manifest, or
    /// download a remote library's catalog again; then say where the library stands.
    pub fn scan_library(&self, library: &str) -> AppResult<LibrarySummary> {
        let handle = self.resolve(library)?;
        if let Some(remote) = self.remote_if(library)? {
            remote.refresh(true).map_err(AppError::from)?;
            return summary(&handle);
        }
        toybox_core::ops::scan::scan_library_catalog(&handle)?;
        summary(&handle)
    }

    /// Remove items and their exclusively owned files from one library. On a remote
    /// library the server deletes; the local cache and files follow.
    pub fn delete_assets(&self, library: &str, asset_ids: Vec<String>) -> AppResult<Deleted> {
        let handle = self.resolve(library)?;
        match self.remote_if(library)? {
            Some(remote) => remote.delete(&asset_ids).map_err(AppError::from)?,
            None => {
                toybox_core::ops::delete::delete_from_library(&handle.root, &handle.cache_path, &asset_ids)?;
            }
        }
        let still_there = Store::for_handle(&handle).existing_asset_ids(&asset_ids)?;
        Ok(Deleted { deleted: asset_ids.into_iter().filter(|id| !still_there.contains(id)).collect(), summary: summary(&handle)? })
    }

    /// The importer commit: validate and upsert `entries` into the library and rebuild
    /// the catalog. Into a remote library the staged files are uploaded and the server commits.
    pub fn merge_seed_entries(&self, library: &str, entries: Vec<Value>) -> AppResult<LibrarySummary> {
        Ok(self.merge_entries(library, entries)?.summary)
    }

    /// [`Host::merge_seed_entries`] that also says which items are in the library now: the ids
    /// of the top-level items the commit wrote (an animation source is listed by its clips). For a
    /// remote library the server answers with the ids of the entries it accepted, which are the
    /// ids the entries carried.
    pub fn merge_entries(&self, library: &str, entries: Vec<Value>) -> AppResult<MergeReport> {
        let handle = self.resolve(library)?;
        if let Some(remote) = self.remote_if(library)? {
            let total = entries.len();
            let (imported, errors) = remote.import_rows(entries).map_err(AppError::from)?;
            if errors.is_empty() {
                return Ok(MergeReport { summary: summary(&handle)?, ids: imported });
            }
            let shown: Vec<String> = errors.iter().take(5).map(|e| format!("{}: {}", e.id.as_deref().unwrap_or("an item"), e.message)).collect();
            let more = errors.len().saturating_sub(5);
            return Err(AppError::msg(format!(
                "{} of {} items could not be imported ({} were added). {}{}",
                errors.len(), total, imported.len(), shown.join(" | "),
                if more > 0 { format!(" | and {more} more") } else { String::new() }
            )));
        }
        let merged = toybox_core::ops::import::merge_entries(&handle, entries)?;
        Ok(MergeReport { summary: summary(&handle)?, ids: merged.assets.into_iter().map(|asset| asset.id).collect() })
    }

    /// Change the library tags and the provenance of items. Returns the top-level items the edit touched
    /// (an edited variant comes inside its parent), as the library's cache now holds them, personal metadata
    /// included. On a remote library the server makes the change and this computer's cache follows.
    pub fn update_asset_metadata(&self, library: &str, edits: &[MetadataEdit]) -> AppResult<Vec<Asset>> {
        if let Some(remote) = self.remote_if(library)? {
            return remote.update_metadata(edits).map_err(AppError::from);
        }
        let handle = self.resolve(library)?;
        update_entries(&handle, edits)
    }

    /// Copy or move items from one library into another; either end may be on this
    /// computer or on a server. `ids` are raw ids of `from_library`. `progress` is called as it goes.
    pub fn transfer_assets(
        &self,
        from_library: &str,
        to_library: &str,
        ids: &[String],
        mode: TransferMode,
        on_conflict: OnConflict,
        include_clips: bool,
        progress: &dyn Fn(TransferProgressEvent),
    ) -> AppResult<TransferReport> {
        let source_remote = self.remote_if(from_library)?;
        let target_remote = self.remote_if(to_library)?;
        let source = if source_remote.is_none() { Some(self.resolve(from_library)?) } else { None };
        let target = if target_remote.is_none() { Some(self.resolve(to_library)?) } else { None };
        let options = TransferOptions { mode, on_conflict, include_clips };
        let from = match (&source_remote, &source) {
            (Some(remote), _) => Side::Remote(remote),
            (None, Some(handle)) => Side::Local(handle),
            (None, None) => return Err(AppError::msg("Unknown source library")),
        };
        let to = match (&target_remote, &target) {
            (Some(remote), _) => Side::Remote(remote),
            (None, Some(handle)) => Side::Local(handle),
            (None, None) => return Err(AppError::msg("Unknown target library")),
        };
        transfer(from, to, ids, options, &|report| {
            progress(TransferProgressEvent { from_library: from_library.to_string(), to_library: to_library.to_string(), progress: report });
        })
    }

    // --- export -------------------------------------------------------------

    /// Copy selected assets as self-contained loose filesets into a target directory.
    pub fn export_copy(&self, library: &str, req: &ExportCopyReq) -> AppResult<ExportReport> {
        toybox_core::ops::export_copy::export_copy(&self.resolve(library)?, req)
    }

    /// Pack each selected asset into a single self-contained `.glb`.
    pub fn export_glb(&self, library: &str, req: &ExportGlbReq) -> AppResult<ExportReport> {
        toybox_core::ops::export_glb::export_glb(&self.resolve(library)?, req)
    }

    /// Resolve and parse one catalog asset's glTF document.
    pub fn read_asset_gltf(&self, library: &str, asset_id: &str) -> AppResult<Value> {
        export_ops::read_asset_gltf(&self.resolve(library)?, asset_id)
    }

    /// Bake one asset to a self-contained `.glb` and return its bytes.
    pub fn assemble_glb_for_asset(&self, library: &str, asset_id: &str) -> AppResult<Vec<u8>> {
        export_ops::assemble_glb_for_asset(&self.resolve(library)?, asset_id)
    }

    /// Write one asset as a loose `.gltf` + `.bin` + textures under `target_dir`.
    pub fn perform_asset_copy(&self, library: &str, asset_id: &str, target_dir: &str, stem: &str, preserve_structure: bool) -> AppResult<Vec<String>> {
        export_ops::perform_asset_copy(&self.resolve(library)?, asset_id, target_dir, stem, preserve_structure)
    }

    /// Plan native output using the same layout and collision rules as the writers.
    pub fn preview_export(&self, library: &str, asset_ids: &[String], format: &str, preserve_structure: bool) -> AppResult<Vec<String>> {
        export_ops::preview_export(&self.resolve(library)?, asset_ids, format, preserve_structure)
    }

    /// Hash the selected file sets so saved exports can detect changes in file contents.
    pub fn asset_fingerprints(&self, library: &str, asset_ids: &[String], all_variants: bool) -> AppResult<BTreeMap<String, String>> {
        export_ops::asset_fingerprints(&self.resolve(library)?, asset_ids, all_variants)
    }

    // --- thumbnails ---------------------------------------------------------

    pub fn save_thumb(&self, library: &str, asset_id: &str, png_bytes: &[u8]) -> AppResult<()> {
        toybox_core::ops::thumbs::save_thumb(&self.resolve(library)?, asset_id, png_bytes)
    }

    pub fn invalidate_thumb(&self, library: &str, asset_id: &str) -> AppResult<()> {
        toybox_core::ops::thumbs::invalidate_thumb(&self.resolve(library)?, asset_id)
    }

    pub fn set_thumb_state(&self, library: &str, asset_id: &str, state: ThumbState, error: Option<String>) -> AppResult<()> {
        toybox_core::ops::thumbs::set_thumb_state(&self.resolve(library)?, asset_id, state, error)
    }

    pub fn clear_thumbs(&self, library: &str, asset_ids: Option<Vec<String>>, preserve_existing: Option<bool>) -> AppResult<()> {
        toybox_core::ops::thumbs::clear_thumbs(&self.resolve(library)?, asset_ids, preserve_existing)
    }

    // --- paths --------------------------------------------------------------

    /// The root folder of a library (for a remote library, where its files are cached).
    pub fn library_root(&self, library: &str) -> AppResult<String> {
        Ok(self.resolve(library)?.root_str())
    }

    /// Map a library-relative path to an absolute path. For a remote library the
    /// file (and the rest of the item it belongs to) is downloaded first.
    pub fn resolve_asset_path(&self, library: &str, rel_path: &str) -> AppResult<String> {
        let handle = self.resolve(library)?;
        handle.ensure_files(std::slice::from_ref(&rel_path.to_string()))?;
        Ok(toybox_core::domain::paths::abs_under_root(&handle.root_str(), rel_path).to_string_lossy().to_string())
    }

    pub fn thumb_dir(&self, library: &str) -> AppResult<String> {
        Ok(self.resolve(library)?.thumbs_dir.to_string_lossy().to_string())
    }

    /// Where an item's thumbnail is. A remote library's shared picture is downloaded
    /// the first time it is asked for; if that fails the path is still returned and
    /// the picture simply does not show.
    pub fn thumb_path(&self, library: &str, asset_id: &str) -> AppResult<String> {
        let handle = self.resolve(library)?;
        let path = handle.thumb_path(asset_id).to_string_lossy().to_string();
        if let Some(link) = handle.remote.clone() {
            let _ = link.files.ensure_thumb(asset_id);
        }
        Ok(path)
    }

    // --- previews, packs, recentering ---------------------------------------

    pub fn read_preview_source(&self, library: &str, relative_path: &str, clip_index: Option<usize>) -> AppResult<Value> {
        let handle = self.resolve(library)?;
        handle.ensure_gltf(relative_path)?;
        preview_source::read_source(&handle.root, relative_path, clip_index)
    }

    pub fn read_preview_pair(&self, library: &str, animation_path: &str, clip_index: usize, model_path: &str) -> AppResult<Value> {
        let handle = self.resolve(library)?;
        handle.ensure_gltf(animation_path)?;
        handle.ensure_gltf(model_path)?;
        preview_source::read_pair(&handle.root, animation_path, clip_index, model_path)
    }

    pub fn compatible_previews(&self, library: &str, asset_id: &str) -> AppResult<Vec<PreviewChoice>> {
        animation_preview::compatible_previews(&self.resolve(library)?, asset_id)
    }

    pub fn read_animation_preview(&self, library: &str, animation_id: &str, model_id: &str) -> AppResult<Value> {
        animation_preview::read_animation_preview(&self.resolve(library)?, animation_id, model_id)
    }

    pub fn load_packs(&self, library: &str) -> AppResult<Vec<PackMeta>> {
        toybox_core::ops::packs::load_packs_for(&self.resolve(library)?)
    }

    /// Correct an asset's origin in place. The fingerprint changes with the file: ask
    /// [`Host::asset_hash_event`] for the new one.
    pub fn recenter_asset(&self, library: &str, asset_id: &str, axis: Axis, align: Align) -> AppResult<Aabb> {
        toybox_core::ops::recenter::recenter_asset(&self.resolve(library)?, asset_id, axis, align)
    }

    /// The current fingerprint of one item as a hashes event (its stored file was edited in place
    /// and its fingerprint recomputed), or `None` when it has none.
    pub fn asset_hash_event(&self, library: &str, asset_id: &str) -> Option<FingerprintHashes> {
        let handle = self.resolve(library).ok()?;
        crate::fingerprint::asset_hash_event(&handle, asset_id)
    }

    // --- importer's FBX bridge ----------------------------------------------

    /// The importer's FBX bridge: shell the configured FBX2glTF to turn one `.fbx`
    /// source into loose glTF and land the produced fileset under
    /// `library/<pack>/<category>/`. Errors with a clear message if no converter is configured.
    pub fn convert_to_gltf(&self, library: &str, src_path: &str, pack: &str, category: &str, stem: &str) -> AppResult<Vec<String>> {
        let library_root = self.resolve(library)?.root;
        let exe = self.load_settings()?.fbx2gltf_path.ok_or_else(|| AppError::msg("no FBX2glTF configured".to_string()))?;
        let temp = std::env::temp_dir();
        toybox_core::ops::import_convert::convert_to_gltf(&library_root, Path::new(&exe), Path::new(src_path), pack, category, stem, &temp)
    }
}
