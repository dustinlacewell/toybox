//! Content fingerprints in the background: the Tauri side. The pass itself is
//! `toybox_host::fingerprint::Fingerprinter`; it runs here on one background thread
//! after the frontend has loaded and shown its catalogs (it calls
//! `start_fingerprinting`), and reports to the window through two events:
//!
//! * `fingerprint-progress` `{ library, done, total, running }`, a few per second
//!   at most; Settings -> Library shows it in the library's panel.
//! * `fingerprint-hashes` `{ library, hashes: { <raw asset id>: <hash> } }`, every
//!   few hundred items and when a library finishes; the frontend patches its
//!   catalog with them, no rescan.

use tauri::{AppHandle, Emitter};
use toybox_host::fingerprint::{FingerprintEvent, FingerprintHashes, FingerprintProgress};

use crate::app_host::host;
use crate::error::{AppError, AppResult};

pub const PROGRESS_EVENT: &str = "fingerprint-progress";
pub const HASHES_EVENT: &str = "fingerprint-hashes";

/// Tell the window the current fingerprints of some items.
pub fn push_hashes(app: &AppHandle, hashes: FingerprintHashes) {
    let _ = app.emit(HASHES_EVENT, hashes);
}

/// Start (or note that the libraries changed for) the background pass over every
/// local library that is missing fingerprints. Returns at once.
#[tauri::command]
pub fn start_fingerprinting(app: AppHandle) -> AppResult<()> {
    let host = host(&app);
    if !host.fingerprinter.begin() {
        return Ok(());
    }
    let worker = host.clone();
    let events = app.clone();
    if let Err(error) = std::thread::Builder::new().name("fingerprint".into()).spawn(move || {
        worker.fingerprinter.run(&worker, &|event| match event {
            FingerprintEvent::Progress(progress) => {
                let _ = events.emit(PROGRESS_EVENT, progress);
            }
            FingerprintEvent::Hashes(hashes) => push_hashes(&events, hashes),
        })
    }) {
        host.fingerprinter.cancel_start();
        return Err(AppError::msg(format!("Could not start fingerprinting: {error}")));
    }
    Ok(())
}

/// Where the pass is for each library it is working on (what the window missed).
#[tauri::command]
pub fn fingerprint_status(app: AppHandle) -> Vec<FingerprintProgress> {
    host(&app).fingerprinter.status()
}
