//! Copying and moving items between any two libraries on this client, local or
//! remote, with the two halves of `toybox_core::ops::transfer` joined by a
//! package:
//!
//! * a local source describes itself (`export_package`) and serves its files
//!   (`local_reader`); a remote source does the same over HTTP
//!   (`POST /assets/export`, `GET /files`, `GET /thumbs`);
//! * a local target writes the package itself (`ingest_package`); a remote
//!   target is sent the package files and runs the same code on the server
//!   (`PUT /uploads/{id}/package/...`, `POST /uploads/{id}/ingest`).
//!
//! Personal metadata (favourites, tags, collections) lives in the caches of
//! this client, so it follows between any two libraries registered here, but is
//! never sent to a server.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::Arc;

use toybox_core::error::{AppError, AppResult};
use toybox_core::infra::library as rules;
use toybox_core::infra::registry::{same_root, LibraryHandle};
use toybox_core::ops::transfer::{
    apply_personal_metadata, complete_move, export_package, fill_user_from_cache, ingest_package, local_reader, sort_report, transfer_between,
    verify_item_present, ExportOptions, IngestOptions, TransferMode, TransferOptions, TransferPackage, TransferProgress, TransferReport,
};

use crate::library::RemoteLibrary;

/// One end of a transfer.
#[derive(Clone, Copy)]
pub enum Side<'a> {
    Local(&'a LibraryHandle),
    Remote(&'a Arc<RemoteLibrary>),
}

impl Side<'_> {
    fn id(&self) -> String {
        match self {
            Side::Local(handle) => handle.id.clone(),
            Side::Remote(remote) => remote.id().to_string(),
        }
    }
}

/// A reader for the files of a package that lives on a remote library.
fn remote_reader<'a>(remote: &'a RemoteLibrary, package: &'a TransferPackage) -> impl Fn(&str) -> AppResult<Box<dyn Read>> + 'a {
    let thumbs: HashMap<&str, &str> = package.assets.iter().flat_map(|asset| asset.thumbs.iter()).map(|thumb| (thumb.file.path.as_str(), thumb.id.as_str())).collect();
    move |path: &str| {
        let client = remote.client();
        if path.starts_with("thumbs/") {
            let id = thumbs.get(path).ok_or_else(|| AppError::msg(format!("The package does not list {path}")))?;
            let bytes = client.thumb(id).map_err(AppError::from)?.ok_or_else(|| AppError::msg("The server has no such thumbnail"))?;
            return Ok(Box::new(Cursor::new(bytes)) as Box<dyn Read>);
        }
        Ok(Box::new(client.file(path).map_err(AppError::from)?.reader) as Box<dyn Read>)
    }
}

/// Copy or move `ids` (raw ids of `source`) into `target`. The report is in the
/// order the ids were given.
pub fn transfer(source: Side, target: Side, ids: &[String], options: TransferOptions, progress: &dyn Fn(TransferProgress)) -> AppResult<TransferReport> {
    if source.id() == target.id() {
        return Err(AppError::msg("Choose a different library to transfer into"));
    }
    if let (Side::Local(a), Side::Local(b)) = (source, target) {
        if same_root(&a.root_str(), &b.root_str()) {
            return Err(AppError::msg("Choose a different library to transfer into"));
        }
        return transfer_between(a, b, ids, options, progress);
    }
    // Who may do what.
    match target {
        Side::Remote(remote) => remote.require_write().map_err(AppError::from)?,
        Side::Local(handle) if !rules::is_writable(&handle.root) => return Err(AppError::msg("The target library is read-only")),
        Side::Local(_) => {}
    }
    if options.mode == TransferMode::Move {
        match source {
            Side::Remote(remote) => remote.require_write().map_err(|e| AppError::msg(format!("Items cannot be moved out of this library: {e}")))?,
            Side::Local(handle) if !rules::is_writable(&handle.root) => {
                return Err(AppError::msg("The source library is read-only, so items cannot be moved out of it. Copy them instead"))
            }
            Side::Local(_) => {}
        }
    }

    // Describe the items.
    let package = match source {
        Side::Local(handle) => export_package(handle, ids, ExportOptions { include_clips: options.include_clips })?,
        Side::Remote(remote) => {
            let mut package = remote.client().export_package(ids, options.include_clips).map_err(AppError::from)?;
            fill_user_from_cache(&mut package, &remote.handle().cache_path);
            package
        }
    };

    // Move the files.
    let local_read;
    let remote_read;
    let read: &dyn Fn(&str) -> AppResult<Box<dyn Read>> = match source {
        Side::Local(handle) => {
            local_read = local_reader(handle);
            &local_read
        }
        Side::Remote(remote) => {
            remote_read = remote_reader(remote, &package);
            &remote_read
        }
    };
    let mut report = match target {
        Side::Local(handle) => ingest_package(handle, &package, IngestOptions { on_conflict: options.on_conflict }, read, progress)?,
        Side::Remote(remote) => {
            let report = remote.upload_package(&package, options.on_conflict, read, progress).map_err(AppError::from)?;
            // The server has the items; make sure this client's replica holds them (the reply
            // normally did that already; this is one small request when it did).
            remote.sync().map_err(AppError::from)?;
            if let Err(error) = apply_personal_metadata(&remote.handle(), &package, &report) {
                eprintln!("toybox: could not carry favourites and tags into {}: {error}", remote.id());
            }
            report
        }
    };

    if options.mode == TransferMode::Move {
        // The ids the remote target's refreshed catalog lists, read once for the whole batch
        // (reading the catalog again for every item made large moves crawl).
        let remote_ids: Option<std::collections::HashSet<String>> = match target {
            Side::Remote(remote) => {
                let asked: Vec<String> = report.items.iter().filter(|item| matches!(item.outcome, toybox_core::ops::transfer::Outcome::Copied | toybox_core::ops::transfer::Outcome::Replaced)).map(|item| item.id.clone()).collect();
                toybox_core::infra::store::Store::for_handle(&remote.handle()).existing_asset_ids(&asked).ok()
            }
            Side::Local(_) => None,
        };
        let verify = |id: &str| -> AppResult<()> {
            match target {
                Side::Local(handle) => verify_item_present(handle, id),
                Side::Remote(_) => {
                    if remote_ids.as_ref().is_some_and(|ids| ids.contains(id)) {
                        Ok(())
                    } else {
                        Err(AppError::msg("it is not in the target library"))
                    }
                }
            }
        };
        let delete = |ids: &[String]| -> AppResult<()> {
            match source {
                Side::Local(handle) => toybox_core::ops::delete::delete_from_library(&handle.root, &handle.cache_path, ids).map(|_| ()),
                Side::Remote(remote) => remote.delete_assets(ids).map(|_| ()).map_err(AppError::from),
            }
        };
        complete_move(&mut report, &verify, &delete);
    }
    sort_report(&mut report, ids);
    Ok(report)
}
