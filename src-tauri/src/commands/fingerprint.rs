//! Content fingerprints in the background. Recognising a copy of an item in
//! another library needs a fingerprint of every item (see
//! `toybox_core::ops::content_hash`); a library that has never been fingerprinted
//! needs one pass over all its files. That pass runs here, on one background
//! thread, one local library at a time, after the frontend has loaded and shown
//! its catalogs (it calls `start_fingerprinting`). Nothing waits for it.
//!
//! Two events tell the window how it is going:
//!
//! * `fingerprint-progress` `{ library, done, total, running }`, a few per second
//!   at most; Settings -> Library shows it in the library's panel.
//! * `fingerprint-hashes` `{ library, hashes: { <raw asset id>: <hash> } }`, every
//!   few hundred items and when a library finishes; the frontend patches its
//!   catalog with them, no rescan.
//!
//! Remote libraries are not hashed here: their server does it and sends the
//! fingerprints with its catalog.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use toybox_core::ops::content_hash::{self, BackfillOptions, Event};

use crate::error::AppResult;
use crate::infra::library;
use crate::infra::registry::{LibraryHandle, LibraryKind};

pub const PROGRESS_EVENT: &str = "fingerprint-progress";
pub const HASHES_EVENT: &str = "fingerprint-hashes";

/// Files hashed side by side. Two keep a spinning disk or a virus scanner busy
/// without taking over the machine.
const THREADS: usize = 2;
/// Hashes are pushed to the window at the latest this often, or at this many.
const PUSH_EVERY: Duration = Duration::from_secs(2);
const PUSH_COUNT: usize = 500;
const PROGRESS_EVERY: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintProgress {
    pub library: String,
    pub done: usize,
    pub total: usize,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FingerprintHashes {
    pub library: String,
    pub hashes: HashMap<String, String>,
}

#[derive(Default)]
struct Inner {
    running: bool,
    /// A library was added (or the window asked again) while a pass was running.
    again: bool,
    status: Vec<FingerprintProgress>,
}

/// Tauri-managed state of the background fingerprinting.
#[derive(Default)]
pub struct Fingerprinter {
    inner: Mutex<Inner>,
    stop: Arc<AtomicBool>,
}

impl Fingerprinter {
    /// Stop at the next item (the app is closing). Finished work is already saved.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set(&self, progress: FingerprintProgress) {
        let mut inner = self.lock();
        inner.status.retain(|other| other.library != progress.library);
        if progress.running {
            inner.status.push(progress);
        }
    }
}

/// Resets `running` however the worker ends.
struct Running<'a>(&'a Fingerprinter);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let mut inner = self.0.lock();
        inner.running = false;
        inner.status.clear();
    }
}

/// Start (or note that the libraries changed for) the background pass over every
/// local library that is missing fingerprints. Returns at once.
#[tauri::command]
pub fn start_fingerprinting(app: AppHandle) -> AppResult<()> {
    let state = app.state::<Fingerprinter>();
    {
        let mut inner = state.lock();
        if inner.running {
            inner.again = true;
            return Ok(());
        }
        inner.running = true;
        inner.again = false;
    }
    let worker = app.clone();
    if let Err(error) = std::thread::Builder::new().name("fingerprint".into()).spawn(move || work(&worker)) {
        state.lock().running = false;
        return Err(crate::error::AppError::msg(format!("Could not start fingerprinting: {error}")));
    }
    Ok(())
}

/// Where the pass is for each library it is working on (what the window missed).
#[tauri::command]
pub fn fingerprint_status(app: AppHandle) -> Vec<FingerprintProgress> {
    app.state::<Fingerprinter>().lock().status.clone()
}

fn work(app: &AppHandle) {
    let state = app.state::<Fingerprinter>();
    let _running = Running(&state);
    loop {
        for id in local_libraries(app) {
            if state.stop.load(Ordering::Relaxed) {
                return;
            }
            fingerprint_library(app, &state, &id);
        }
        let mut inner = state.lock();
        if inner.again && !state.stop.load(Ordering::Relaxed) {
            inner.again = false;
            continue;
        }
        return;
    }
}

/// Ids of the local libraries, the default first.
fn local_libraries(app: &AppHandle) -> Vec<String> {
    let registry = library::snapshot(app);
    let mut ids: Vec<String> = registry.libraries.iter().filter(|entry| entry.kind == LibraryKind::Local).map(|entry| entry.id.clone()).collect();
    if let Some(at) = registry.default.as_ref().and_then(|default| ids.iter().position(|id| id == default)) {
        let default = ids.remove(at);
        ids.insert(0, default);
    }
    ids
}

/// Collects hashes and sends them on in batches.
struct Pusher<'a> {
    app: &'a AppHandle,
    library: &'a str,
    pending: HashMap<String, String>,
    last: Instant,
}

impl Pusher<'_> {
    fn add(&mut self, hashes: &[(String, String)]) {
        self.pending.extend(hashes.iter().cloned());
        if self.pending.len() >= PUSH_COUNT || self.last.elapsed() >= PUSH_EVERY {
            self.flush();
        }
    }

    fn flush(&mut self) {
        self.last = Instant::now();
        if self.pending.is_empty() {
            return;
        }
        let hashes = std::mem::take(&mut self.pending);
        let _ = self.app.emit(HASHES_EVENT, FingerprintHashes { library: self.library.to_string(), hashes });
    }
}

fn fingerprint_library(app: &AppHandle, state: &Fingerprinter, id: &str) {
    let Ok(handle) = library::resolve(app, id) else { return };
    // An unplugged drive or a library that cannot be read is simply not worked on.
    if !handle.root.is_dir() {
        return;
    }
    // Make sure the library is opened (migrated, with a current catalog); then read only what lacks a hash.
    if toybox_core::ops::catalog::open_library(&handle).is_err() || !toybox_core::infra::store::Store::for_handle(&handle).catalog_ready() {
        return;
    }
    let Ok(catalog) = content_hash::unhashed_catalog(&handle) else { return };
    if content_hash::count_unhashed(&catalog) == 0 {
        let _ = content_hash::prune_hashes(&handle);
        return;
    }
    let report = |progress: FingerprintProgress| {
        state.set(progress.clone());
        let _ = app.emit(PROGRESS_EVENT, progress);
    };
    let mut pusher = Pusher { app, library: id, pending: HashMap::new(), last: Instant::now() };
    let mut last_progress = Instant::now() - PROGRESS_EVERY;
    let mut total = 0;
    let options = BackfillOptions { threads: THREADS, prune: false, ..Default::default() };
    let result = content_hash::backfill(&handle.root, &content_hash::cache_path(&handle), &catalog, &state.stop, &options, &mut |event| match event {
        Event::Started { total: count } => {
            total = count;
            report(FingerprintProgress { library: id.to_string(), done: 0, total, running: true });
        }
        Event::Progress { done, total } => {
            if last_progress.elapsed() >= PROGRESS_EVERY {
                last_progress = Instant::now();
                report(FingerprintProgress { library: id.to_string(), done, total, running: true });
            }
        }
        Event::Saved { hashes } => pusher.add(hashes),
    });
    pusher.flush();
    let _ = content_hash::prune_hashes(&handle);
    // `running: false` also clears the line in Settings. A failed pass is not shown:
    // what was saved stays, and the next start tries the rest.
    let _ = result;
    report(FingerprintProgress { library: id.to_string(), done: total, total, running: false });
}

/// Tell the window the current fingerprint of one item (its stored file was edited
/// in place and its fingerprint recomputed).
pub fn push_asset_hash(app: &AppHandle, handle: &LibraryHandle, asset_id: &str) {
    let mut found = toybox_core::infra::store::Store::for_handle(handle).load_top_assets(&[asset_id.to_string()]).unwrap_or_default();
    content_hash::fill_assets(handle, &mut found);
    let Some(hash) = found.first().and_then(|asset| asset.content_hash.clone()) else { return };
    let _ = app.emit(HASHES_EVENT, FingerprintHashes { library: handle.id.clone(), hashes: HashMap::from([(asset_id.to_string(), hash)]) });
}
