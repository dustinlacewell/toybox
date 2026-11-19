//! One remote library as this client sees it: the connection (address, token,
//! role), the replica of the catalog kept in the library's usual app-data folder
//! (kept up to date by following the server's revision feed), the file cache, and
//! the shared thumbnails. [`RemoteLibrary::handle`] turns it into the
//! `LibraryHandle` the rest of the app already works with.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex, RwLock, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use toybox_core::config;
use toybox_core::domain::catalog_model::{Asset, Catalog};
use toybox_core::domain::merge::{absorb, is_old_member, old_member_keys};
use toybox_core::error::{AppError, AppResult};
use toybox_core::infra::fsio;
use toybox_core::infra::library::{RemoteRole, RemoteStatus};
use toybox_core::infra::registry::{FileProvider, LibraryEntry, LibraryHandle, RemoteLink};
use toybox_core::infra::store::{BulkWriter, ChangesPage, Delta, ItemPut, Store};
use toybox_core::ops::catalog::load_catalog_inner;
use toybox_core::ops::metadata::MetadataEdit;

use crate::client::{Client, API_LEVEL};
use crate::error::{RemoteError, RemoteResult};
use crate::files::{valid_rel, FileCache};
use crate::progress::{Progress, Sink};
use crate::types::Info;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// Marks a picture this client rendered and offered itself.
const OURS: &str = "local";

/// What is remembered between runs (`<app-data>/libraries/<id>/remote.json`).
/// Never the token.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    #[serde(default)]
    server_name: String,
    #[serde(default)]
    role: Option<RemoteRole>,
    /// How the server's version fits this Toybox, as of the last time it answered (or refused).
    #[serde(default)]
    compat: Option<toybox_core::infra::library::RemoteCompat>,
    /// Shared pictures the server had at the last sync: asset id -> version.
    #[serde(default)]
    shared_thumbs: BTreeMap<String, String>,
    /// The version of the picture held locally, by asset id (`local` = rendered here).
    #[serde(default)]
    local_thumbs: BTreeMap<String, String>,
}

struct State {
    saved: Saved,
    offline: bool,
    reason: Option<String>,
}

/// Counters for diagnostics and tests.
#[derive(Debug, Default)]
pub struct Stats {
    /// Times a sync took changes from the server's feed (a sync that found nothing new
    /// does not count, nor does applying the answer to this client's own write).
    pub catalog_downloads: AtomicU64,
    pub file_downloads: AtomicU64,
    pub thumb_downloads: AtomicU64,
    pub thumb_uploads: AtomicU64,
}

struct Uploader {
    sender: Mutex<Option<Sender<(String, Vec<u8>)>>>,
    pending: Arc<(Mutex<usize>, Condvar)>,
}

pub struct RemoteLibrary {
    pub(crate) id: String,
    pub(crate) url: String,
    pub(crate) app_data: PathBuf,
    pub(crate) entry: LibraryEntry,
    pub(crate) data_dir: PathBuf,
    pub(crate) cache: FileCache,
    client: RwLock<Client>,
    state: Mutex<State>,
    /// One sync of the replica at a time.
    sync: Mutex<()>,
    /// Rows of the feed asked for at a time (a setting only so tests can use small pages).
    page_rows: std::sync::atomic::AtomicUsize,
    me: Weak<RemoteLibrary>,
    pub(crate) progress: Option<Sink>,
    pub stats: Stats,
    uploader: Uploader,
}

impl std::fmt::Debug for RemoteLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteLibrary").field("id", &self.id).field("url", &self.url).finish()
    }
}

/// Rows of the revision feed asked for at a time.
const PAGE_ROWS: usize = 5000;

/// Where the replica stands in the server's revision feed (kept in the replica's
/// own database, so it moves with the rows it describes).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Cursor {
    revision: i64,
    /// Inside that revision, when the last page ended in the middle of one.
    after: Option<String>,
    /// The server database the rows came from.
    db_id: String,
}

fn require_current(info: &Info) -> RemoteResult<()> {
    if info.api_version < API_LEVEL {
        return Err(RemoteError::ServerTooOld { found: info.api_version, needed: API_LEVEL, server_version: info.server_version.clone() });
    }
    Ok(())
}

fn check_schema(page: &ChangesPage) -> RemoteResult<()> {
    if page.schema_version != config::SCHEMA_VERSION {
        return Err(RemoteError::Protocol(format!(
            "the server's catalog format is version {}, but this version of Toybox uses {}. Update whichever is older",
            page.schema_version,
            config::SCHEMA_VERSION
        )));
    }
    Ok(())
}

/// Ids of assets (and variants) the server sent with no provenance: it owns the
/// provenance of its items, so such an item has had it cleared; carrying the old
/// provenance over from a row it absorbs would bring it back.
fn note_cleared(assets: &[Asset], out: &mut HashSet<String>) {
    for asset in assets {
        if asset.user.provenance.is_none() {
            out.insert(asset.id.clone());
        }
        note_cleared(&asset.variants, out);
    }
}

fn forget_provenance(assets: &mut [Asset], cleared: &HashSet<String>) {
    for asset in assets {
        if cleared.contains(&asset.id) {
            if let Some(old) = asset.user.provenance.take() {
                let derived = old.tags();
                asset.user.tags.retain(|tag| !derived.contains(tag));
            }
        }
        forget_provenance(&mut asset.variants, cleared);
    }
}

fn collect_ids(asset: &Asset, out: &mut Vec<String>) {
    out.push(asset.id.clone());
    for variant in &asset.variants {
        collect_ids(variant, out);
    }
}

fn collect_rig_ids(asset: &Asset, out: &mut Vec<String>) {
    out.extend(asset.rigs.iter().map(|binding| binding.rig_id.clone()));
    for variant in &asset.variants {
        collect_rig_ids(variant, out);
    }
}

fn not_a_png(png: &[u8]) -> bool {
    png.len() < PNG_SIGNATURE.len() || png[..8] != PNG_SIGNATURE
}

impl RemoteLibrary {
    /// Open a remote library from its registry entry and token. No network is used.
    pub fn open(app_data: &Path, files_root: &Path, entry: &LibraryEntry, token: &str, progress: Option<Sink>) -> RemoteResult<Arc<RemoteLibrary>> {
        let url = entry.url.clone().ok_or_else(|| RemoteError::Local(format!("{} has no server address.", entry.name)))?;
        let data_dir = toybox_core::infra::registry::library_dir(app_data, &entry.id);
        let saved = std::fs::read(data_dir.join("remote.json")).ok().and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok()).unwrap_or_default();
        let library = Arc::new_cyclic(|me| RemoteLibrary {
            id: entry.id.clone(),
            client: RwLock::new(Client::new(&url, token)),
            url,
            app_data: app_data.to_path_buf(),
            entry: entry.clone(),
            data_dir,
            cache: FileCache::new(files_root.to_path_buf()),
            state: Mutex::new(State { saved, offline: false, reason: None }),
            sync: Mutex::new(()),
            page_rows: std::sync::atomic::AtomicUsize::new(PAGE_ROWS),
            me: me.clone(),
            progress,
            stats: Stats::default(),
            uploader: Uploader { sender: Mutex::new(None), pending: Arc::new((Mutex::new(0), Condvar::new())) },
        });
        Ok(library)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Ask the server for this many rows of its revision feed at a time.
    pub fn set_page_rows(&self, rows: usize) {
        self.page_rows.store(rows.max(1), Ordering::Relaxed);
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn client(&self) -> Client {
        self.client.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Use another token from now on (after the user changed it).
    pub fn set_token(&self, token: &str) {
        let mut client = self.client.write().unwrap_or_else(|p| p.into_inner());
        *client = client.with_token(token);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn save_state(&self) {
        let bytes = {
            let state = self.lock();
            serde_json::to_vec_pretty(&state.saved)
        };
        if let Ok(bytes) = bytes {
            let _ = fsio::write_bytes_atomic(&self.data_dir.join("remote.json"), &bytes);
        }
    }

    /// Record what a connection check just learned, without another request.
    pub fn remember(&self, server_name: &str, role: RemoteRole) {
        {
            let mut state = self.lock();
            state.saved.server_name = server_name.to_string();
            state.saved.role = Some(role);
            state.offline = false;
            state.reason = None;
        }
        self.save_state();
    }

    /// Keep how the server's version fits this Toybox (from a connection check).
    pub fn remember_compat(&self, compat: toybox_core::infra::library::RemoteCompat) {
        self.lock().saved.compat = Some(compat);
        self.save_state();
    }

    pub fn files_root(&self) -> &Path {
        self.cache.root()
    }

    pub fn role(&self) -> RemoteRole {
        self.lock().saved.role.unwrap_or(RemoteRole::Read)
    }

    pub fn server_name(&self) -> String {
        self.lock().saved.server_name.clone()
    }

    pub fn is_offline(&self) -> bool {
        self.lock().offline
    }

    /// Whether items may be added to, deleted from and transferred into this library now.
    pub fn can_write(&self) -> bool {
        let state = self.lock();
        !state.offline && state.saved.role.is_some_and(|role| role >= RemoteRole::Write)
    }

    /// Fail with a plain sentence unless this token may change the library.
    pub fn require_write(&self) -> RemoteResult<()> {
        let state = self.lock();
        if state.offline {
            return Err(RemoteError::Local("This library is offline, so it can only be browsed. Try again when the server can be reached.".into()));
        }
        if !state.saved.role.is_some_and(|role| role >= RemoteRole::Write) {
            return Err(RemoteError::Forbidden("This library is read-only with your access token. Ask the person who runs the server for a token that can edit.".into()));
        }
        Ok(())
    }

    pub fn status(&self) -> RemoteStatus {
        let state = self.lock();
        RemoteStatus {
            compat: state.saved.compat.clone(),
            role: state.saved.role.unwrap_or(RemoteRole::Read),
            server_name: state.saved.server_name.clone(),
            offline: state.offline,
            offline_reason: state.reason.clone(),
            has_cache: toybox_core::infra::store::remote_cache_present(&self.data_dir),
        }
    }

    /// The `LibraryHandle` the app works through: file cache as `root`, the usual
    /// per-library cache and thumbnails, and this library as its file provider.
    pub fn handle(&self) -> LibraryHandle {
        let me: Arc<RemoteLibrary> = self.me.upgrade().expect("a RemoteLibrary is always inside its Arc");
        LibraryHandle::remote(&self.app_data, &self.entry, self.cache.root(), RemoteLink { url: self.url.clone(), files: me })
    }

    fn emit(&self, progress: Progress) {
        if let Some(sink) = &self.progress {
            sink(&progress);
        }
    }

    pub(crate) fn emit_progress(&self, operation: &'static str, done_files: u64, total_files: u64, done_bytes: u64, total_bytes: u64, label: Option<String>) {
        self.emit(Progress { library: self.id.clone(), operation, done_files, total_files, done_bytes, total_bytes, label });
    }

    pub(crate) fn go_offline(&self, error: &RemoteError) {
        let mut state = self.lock();
        state.offline = true;
        state.reason = Some(error.to_string());
    }

    fn note_online(&self) {
        let mut state = self.lock();
        state.offline = false;
        state.reason = None;
    }

    // ---- connecting and the catalog ----------------------------------------------

    /// Ask the server who we are. Updates the role and name; a server that cannot
    /// be reached (or refuses the token, or is too old) makes the library offline instead of failing.
    pub fn connect(&self) -> RemoteStatus {
        let asked = self.client().info();
        // The versions are worth remembering even when the server is then refused.
        let compat = match &asked {
            Ok(info) => Some(crate::compat::assess(info, crate::compat::APP_VERSION)),
            Err(error) => crate::compat::from_error(error),
        };
        if compat.is_some() {
            self.lock().saved.compat = compat;
            self.save_state();
        }
        match asked.and_then(|info| require_current(&info).map(|()| info)) {
            Ok(info) => {
                {
                    let mut state = self.lock();
                    state.saved.server_name = info.name;
                    state.saved.role = Some(info.role);
                    state.offline = false;
                    state.reason = None;
                }
                self.save_state();
            }
            Err(error) => self.go_offline(&error),
        }
        self.status()
    }

    /// The replica as it is, when the server cannot be reached: usable if a catalog was ever saved.
    fn cache_ready(&self, error: RemoteError) -> RemoteResult<()> {
        let handle = self.handle();
        if toybox_core::infra::store::remote_cache_present(&self.data_dir) {
            toybox_core::ops::catalog::open_library(&handle)?;
            if Store::for_handle(&handle).catalog_exists() {
                return Ok(());
            }
        }
        Err(error)
    }

    /// The library's catalog: the replica is brought up to date from the server's
    /// revision feed (only what changed is downloaded and written; personal metadata
    /// this client keeps is untouched), and the whole catalog is read back from it.
    /// `force` downloads everything again. When the server cannot be reached the
    /// replica's catalog is returned and the library is marked offline; with no
    /// replica the error is returned.
    pub fn refresh_catalog(&self, force: bool) -> RemoteResult<Catalog> {
        self.refresh(force)?;
        load_catalog_inner(&self.handle())?.ok_or_else(|| RemoteError::Local("The catalog could not be read back from the replica.".into()))
    }

    /// [`RemoteLibrary::refresh_catalog`] without reading the catalog back: the replica is brought up to
    /// date (or, when the server cannot be reached, left as it is and the library marked offline).
    /// The window reads the replica through queries.
    pub fn refresh(&self, force: bool) -> RemoteResult<()> {
        let client = self.client();
        match client.info().and_then(|info| require_current(&info).map(|()| info)) {
            Ok(info) => {
                {
                    let mut state = self.lock();
                    state.saved.server_name = info.name;
                    state.saved.role = Some(info.role);
                }
                self.note_online();
            }
            Err(error) if error.is_connectivity() || error.is_access() || matches!(error, RemoteError::Protocol(_) | RemoteError::ServerTooOld { .. } | RemoteError::ClientTooOld { .. }) => {
                self.go_offline(&error);
                return self.cache_ready(error);
            }
            Err(error) => return Err(error),
        }
        match self.pull(force) {
            // Pictures are a courtesy: a failure here never fails the catalog.
            Ok(thumb_revision) => {
                let _ = self.sync_thumbs_from(Some(thumb_revision), force);
            }
            Err(error) if error.is_connectivity() => {
                self.go_offline(&error);
                return self.cache_ready(error);
            }
            Err(error) => return Err(error),
        }
        self.save_state();
        Ok(())
    }

    /// Bring the replica up to date without reading the whole catalog back:
    /// follow the feed, then the pictures.
    pub fn sync(&self) -> RemoteResult<()> {
        let began = std::time::Instant::now();
        let thumb_revision = self.pull(false)?;
        let pulled = began.elapsed();
        // Saves its own state when the pictures changed; nothing else here needs saving.
        let _ = self.sync_thumbs_from(Some(thumb_revision), false);
        if std::env::var("TOYBOX_TRACE_SYNC").is_ok() {
            eprintln!("  sync: pull {:.0} ms, pictures {:.0} ms", pulled.as_secs_f64() * 1000.0, (began.elapsed() - pulled).as_secs_f64() * 1000.0);
        }
        Ok(())
    }

    fn cursor(&self, store: &Store) -> Option<Cursor> {
        let revision: i64 = store.meta("remoteRevision")?.parse().ok()?;
        Some(Cursor { revision, after: store.meta("remoteAfter").filter(|after| !after.is_empty()), db_id: store.meta("remoteDbId").unwrap_or_default() })
    }

    /// Follow the server's revision feed from where the replica is, a page at a time,
    /// writing each page to the replica as it arrives (so an interrupted download
    /// goes on from the page it reached). With no cursor, or `from_scratch`, or when
    /// the server says it cannot answer for the cursor (`reset`), the replica is
    /// emptied first (favourites and tags wait in `user.db`) and downloaded from 0.
    /// Returns the server's thumbnail revision.
    pub(crate) fn pull(&self, from_scratch: bool) -> RemoteResult<i64> {
        self.pull_pages(from_scratch, None)
    }

    /// Follow the feed for at most `pages` pages, as an interrupted download would
    /// stop (for tests); the next sync goes on from there.
    #[doc(hidden)]
    pub fn sync_pages(&self, pages: usize) -> RemoteResult<()> {
        self.pull_pages(false, Some(pages)).map(|_| ())
    }

    fn pull_pages(&self, from_scratch: bool, max_pages: Option<usize>) -> RemoteResult<i64> {
        let _sync = self.sync.lock().unwrap_or_else(|p| p.into_inner());
        let handle = self.handle();
        toybox_core::ops::catalog::open_library(&handle)?;
        let store = Store::for_handle(&handle);
        let client = self.client();
        let mut cursor = if from_scratch { None } else { self.cursor(&store) };
        let (mut resets, mut received, mut pages) = (0, false, 0);
        // A download that is going on in bulk mode (see `Store::begin_bulk`) goes on in it.
        let mut bulk = cursor.is_some() && store.is_bulk();
        let rows = self.page_rows.load(Ordering::Relaxed);
        // The next page is asked for while this one is written (the cursor for it is in the page that just arrived).
        let thumb_revision = std::thread::scope(|scope| -> RemoteResult<i64> {
            let mut ahead: Option<std::thread::ScopedJoinHandle<'_, RemoteResult<ChangesPage>>> = None;
            let mut writer: Option<BulkWriter> = if bulk { Some(store.bulk_writer()?) } else { None };
            loop {
                let (since, after, known) = match &cursor {
                    Some(held) => (held.revision, held.after.clone(), held.db_id.clone()),
                    None => (0, None, String::new()),
                };
                let page = match ahead.take() {
                    Some(handle) => handle.join().unwrap_or_else(|_| Err(RemoteError::Protocol("a download thread stopped".into())))?,
                    None => client.changes(since, after.as_deref(), rows, &known)?,
                };
                if page.reset {
                    resets += 1;
                    if resets > 2 {
                        return Err(RemoteError::Protocol("the server keeps asking to start again".into()));
                    }
                    drop(writer.take());
                    self.clear_for_download(&store, &mut bulk)?;
                    self.lock().saved.shared_thumbs.clear();
                    cursor = None;
                    continue;
                }
                check_schema(&page)?;
                if cursor.is_none() {
                    // From zero: a download of more than a page goes in bulk mode (rows first, indexes at the end).
                    if page.more {
                        drop(writer.take());
                        store.begin_bulk()?;
                        bulk = true;
                        writer = Some(store.bulk_writer()?);
                    } else {
                        self.clear_for_download(&store, &mut bulk)?;
                    }
                }
                received |= !page.assets.is_empty() || !page.deleted.is_empty();
                if page.more && !max_pages.is_some_and(|max| pages + 1 >= max) {
                    let (next, after, db_id) = (page.next, page.after.clone(), page.db_id.clone());
                    let client = client.clone();
                    ahead = Some(scope.spawn(move || client.changes(next, after.as_deref(), rows, &db_id)));
                }
                self.apply_page(&store, &page, cursor.as_ref(), bulk, writer.as_mut())?;
                cursor = Some(Cursor { revision: page.next, after: page.after.clone(), db_id: page.db_id.clone() });
                pages += 1;
                if !page.more {
                    if bulk {
                        let began = std::time::Instant::now();
                        drop(writer.take());
                        store.finish_bulk()?;
                        if std::env::var("TOYBOX_TRACE_SYNC").is_ok() {
                            eprintln!("  finish_bulk {:.0} ms", began.elapsed().as_secs_f64() * 1000.0);
                        }
                    }
                    break Ok(page.thumb_revision);
                }
                if max_pages.is_some_and(|max| pages >= max) {
                    break Ok(page.thumb_revision);
                }
            }
        })?;
        if received {
            self.stats.catalog_downloads.fetch_add(1, Ordering::Relaxed);
        }
        Ok(thumb_revision)
    }

    /// Empty the replica for a download from zero that fits one page (a replica left in bulk mode is made whole first).
    fn clear_for_download(&self, store: &Store, bulk: &mut bool) -> RemoteResult<()> {
        if store.is_bulk() {
            store.begin_bulk()?;
            store.finish_bulk()?;
        }
        *bulk = false;
        store.clear_replica()?;
        Ok(())
    }

    /// Write one page of the feed to the replica, and the cursor with it, in one
    /// transaction. Only the rows named in the page (and the rows its assets absorb)
    /// are read or written. Applying a page twice changes nothing. With `bulk` the
    /// replica is in bulk mode (see `Store::begin_bulk`).
    fn apply_page(&self, store: &Store, page: &ChangesPage, prior: Option<&Cursor>, bulk: bool, writer: Option<&mut BulkWriter>) -> RemoteResult<()> {
        let moved = prior.is_none_or(|held| held.revision != page.next || held.after != page.after || held.db_id != page.db_id);
        if page.assets.is_empty() && page.deleted.is_empty() && !moved {
            return Ok(());
        }
        let trace = std::env::var("TOYBOX_TRACE_SYNC").is_ok();
        let began = std::time::Instant::now();
        let mut cleared = HashSet::new();
        note_cleared(&page.assets, &mut cleared);
        // What this computer keeps for the assets (by id), then what it kept for rows
        // the new assets absorb (a group that now holds what used to be separate rows).
        let mut assets = store.overlay_personal(page.assets.clone())?;
        let (mut ids, mut paths) = (Vec::new(), Vec::new());
        for asset in &assets {
            let (more_ids, more_paths) = old_member_keys(asset);
            ids.extend(more_ids);
            paths.extend(more_paths);
        }
        ids.sort();
        ids.dedup();
        paths.sort();
        paths.dedup();
        if trace {
            eprintln!("  page {} rows: overlay {:.0} ms", page.assets.len(), began.elapsed().as_secs_f64() * 1000.0);
        }
        let stored = store.load_assets_matching(&ids, &paths)?;
        if trace {
            eprintln!("  page: matching loaded {:.0} ms", began.elapsed().as_secs_f64() * 1000.0);
        }
        let fresh_ids: HashSet<String> = assets.iter().map(|asset| asset.id.clone()).collect();
        let mut absorbed: Vec<String> = Vec::new();
        let mut done: Vec<Asset> = Vec::new();
        // Only an asset with variants can absorb other rows (`is_old_member`); without them, nothing is looked through.
        let any_variants = assets.iter().any(|asset| !asset.variants.is_empty());
        for asset in assets.iter_mut() {
            let members: Vec<&Asset> =
                if asset.variants.is_empty() { Vec::new() } else { stored.iter().chain(done.iter()).filter(|old| old.id != asset.id && is_old_member(asset, old)).collect() };
            absorbed.extend(members.iter().filter(|old| !fresh_ids.contains(&old.id)).map(|old| old.id.clone()));
            absorb(asset, &members);
            forget_provenance(std::slice::from_mut(asset), &cleared);
            if any_variants {
                done.push(asset.clone());
            }
        }
        drop(done);

        let mut removed: Vec<String> = page.deleted.clone();
        removed.extend(absorbed);
        removed.sort();
        removed.dedup();
        removed.retain(|id| !fresh_ids.contains(id));
        // What the removed items own, read before they go (their files and pictures are cleaned up after).
        let doomed: Vec<Asset> = if removed.is_empty() { Vec::new() } else { store.load_top_assets(&removed)? };
        let mut kept = Vec::new();
        for asset in &assets {
            collect_ids(asset, &mut kept);
        }
        let mut named = removed.clone();
        for asset in &doomed {
            collect_ids(asset, &mut named);
        }
        // A choice that named a removed item points at nothing now, unless it is still in the library.
        named.retain(|id| !kept.contains(id));
        let puts: Vec<ItemPut> = assets
            .into_iter()
            .map(|asset| {
                let mut used = Vec::new();
                collect_rig_ids(&asset, &mut used);
                ItemPut { id: asset.id.clone(), entry: None, assets: vec![asset], rigs: page.rigs.iter().filter(|rig| used.contains(&rig.id)).cloned().collect() }
            })
            .collect();
        let set_meta = vec![
            ("catalogSchema".to_string(), config::SCHEMA_VERSION.to_string()),
            ("remoteRevision".to_string(), page.next.to_string()),
            ("remoteAfter".to_string(), page.after.clone().unwrap_or_default()),
            ("remoteDbId".to_string(), page.db_id.clone()),
        ];
        if trace {
            eprintln!("  page: delta built {:.0} ms", began.elapsed().as_secs_f64() * 1000.0);
        }
        let delta = Delta { puts, remove_items: removed, clear_previews: named, set_meta, bulk, ..Default::default() };
        match writer {
            Some(writer) => writer.apply(&delta)?,
            None => store.apply(&delta)?,
        };
        if trace {
            eprintln!("  page: applied {:.0} ms", began.elapsed().as_secs_f64() * 1000.0);
        }
        if !doomed.is_empty() {
            self.forget_items(store, &doomed);
        }
        Ok(())
    }

    /// Apply the rows a write's reply carries, when nothing else happened to the
    /// library between this replica's last sync and the write (`base` is where the
    /// replica stands) and the reply holds everything. False when it has to be pulled.
    fn apply_reply(&self, base: i64, page: &ChangesPage) -> RemoteResult<bool> {
        if page.more || page.reset || page.since != base {
            return Ok(false);
        }
        let _sync = self.sync.lock().unwrap_or_else(|p| p.into_inner());
        let handle = self.handle();
        toybox_core::ops::catalog::open_library(&handle)?;
        let store = Store::for_handle(&handle);
        let Some(held) = self.cursor(&store) else { return Ok(false) };
        if store.is_bulk() {
            // The first download is not finished: the reply is not applied on top of it; the pull goes on from its cursor.
            return Ok(false);
        }
        if held.revision != base || held.after.is_some() || held.db_id != page.db_id {
            return Ok(false);
        }
        check_schema(page)?;
        self.apply_page(&store, page, Some(&held), false, None)?;
        Ok(true)
    }

    /// After a write of this client: apply the reply directly if the replica was
    /// current when the write began, otherwise follow the feed.
    pub(crate) fn catch_up(&self, base: i64, changes: Option<ChangesPage>) -> RemoteResult<()> {
        if let Some(page) = &changes {
            if self.apply_reply(base, page)? {
                return Ok(());
            }
        }
        let thumb_revision = self.pull(false)?;
        let _ = self.sync_thumbs_from(Some(thumb_revision), false);
        Ok(())
    }

    /// The files of the item (or variant) that owns `rel` alone, so that asking for
    /// one image of a model brings the whole model. A file used by several items
    /// (a shared texture) belongs to none of them in particular: asking for it
    /// must not drag in an unrelated item. Read from the database for this one file.
    fn group_of(&self, store: &Store, rel: &str) -> Option<Vec<String>> {
        let owners = store.content_file_owners(&[rel.to_string()]).ok()?.remove(rel)?;
        let [owner] = owners.as_slice() else { return None };
        let asset = store.load_top_assets(std::slice::from_ref(owner)).ok()?.into_iter().next()?;
        fn units<'a>(asset: &'a Asset, out: &mut Vec<Vec<String>>, rel: &str) {
            let mut files = Vec::new();
            asset.content_files(&mut files);
            files.sort();
            files.dedup();
            if files.iter().any(|file| file == rel) {
                out.push(files);
            }
            for variant in &asset.variants {
                units(variant, out, rel);
            }
        }
        let mut holding = Vec::new();
        units(&asset, &mut holding, rel);
        if holding.len() == 1 { holding.pop() } else { None }
    }

    /// Download what is missing of `rels`, and of the items they belong to.
    pub fn ensure(&self, rels: &[String]) -> RemoteResult<()> {
        if let Some(bad) = rels.iter().find(|rel| !valid_rel(rel)) {
            return Err(RemoteError::Local(format!("Not a file of this library: {bad}")));
        }
        let store = Store::for_handle(&self.handle());
        let mut wanted: Vec<&String> = Vec::new();
        let mut extra: Vec<String> = Vec::new();
        for rel in rels {
            if !wanted.contains(&rel) {
                wanted.push(rel);
            }
        }
        for rel in rels {
            for sibling in self.group_of(&store, rel).into_iter().flatten() {
                if !wanted.iter().any(|w| **w == sibling) && !extra.contains(&sibling) {
                    extra.push(sibling);
                }
            }
        }
        let missing: Vec<&String> = wanted.iter().copied().chain(extra.iter()).filter(|rel| !self.cache.has(rel)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        let client = self.client();
        let total = missing.len() as u64;
        let mut bytes = 0u64;
        for (index, rel) in missing.iter().enumerate() {
            let required = wanted.contains(rel);
            match self.cache.ensure(&client, rel) {
                Ok(count) => {
                    bytes += count;
                    if count > 0 {
                        self.stats.file_downloads.fetch_add(1, Ordering::Relaxed);
                    }
                }
                // The rest of an item is fetched along the way; only what was asked for must arrive.
                Err(error) if !required && !error.is_connectivity() => {}
                Err(error) => {
                    if error.is_connectivity() {
                        self.go_offline(&error);
                    }
                    return Err(error);
                }
            }
            self.emit_progress("download", index as u64 + 1, total, bytes, 0, Some((**rel).clone()));
        }
        Ok(())
    }

    // ---- shared thumbnails -------------------------------------------------------

    /// Learn which items have a shared picture; forget local copies the server replaced.
    /// Only the pictures saved since the last sync are listed, once the first one
    /// has listed them all.
    pub fn sync_thumbs(&self) -> RemoteResult<()> {
        self.sync_thumbs_from(None, false)
    }

    /// [`RemoteLibrary::sync_thumbs`], skipped when the replica already holds the
    /// server's thumbnail revision `known`; `full` lists every picture again.
    fn sync_thumbs_from(&self, known: Option<i64>, full: bool) -> RemoteResult<()> {
        let handle = self.handle();
        let store = Store::for_handle(&handle);
        let held = if full { None } else { store.meta("remoteThumbRevision").and_then(|text| text.parse::<i64>().ok()) };
        if held.is_some() && held == known {
            return Ok(());
        }
        let list = self.client().thumbs_since(held)?;
        {
            let mut state = self.lock();
            let incoming: BTreeMap<String, String> = list.thumbs.into_iter().map(|t| (t.id, t.version)).collect();
            let ids: Vec<String> = state.saved.local_thumbs.keys().cloned().collect();
            for id in ids {
                let local = state.saved.local_thumbs[&id].clone();
                match incoming.get(&id) {
                    Some(version) if local == OURS => {
                        state.saved.local_thumbs.insert(id, version.clone());
                    }
                    Some(version) if *version != local => {
                        let _ = std::fs::remove_file(handle.thumb_path(&id));
                        state.saved.local_thumbs.remove(&id);
                    }
                    _ => {}
                }
            }
            // A picture already on disk from before this was tracked is taken as current.
            for (id, version) in &incoming {
                if !state.saved.local_thumbs.contains_key(id) && handle.thumb_path(id).is_file() {
                    state.saved.local_thumbs.insert(id.clone(), version.clone());
                }
            }
            if held.is_none() {
                state.saved.shared_thumbs = incoming;
            } else {
                state.saved.shared_thumbs.extend(incoming);
            }
        }
        store.set_meta("remoteThumbRevision", &list.thumb_revision.to_string())?;
        self.save_state();
        Ok(())
    }

    fn upload_thumb(&self, id: &str, png: &[u8]) {
        if self.client().put_thumb(id, png).is_ok() {
            self.stats.thumb_uploads.fetch_add(1, Ordering::Relaxed);
            let mut state = self.lock();
            state.saved.shared_thumbs.insert(id.to_string(), OURS.to_string());
            state.saved.local_thumbs.insert(id.to_string(), OURS.to_string());
        }
    }

    /// Wait until the thumbnail uploads queued so far have been tried. For tests
    /// and for shutting down; the app never waits for them.
    pub fn flush_thumb_uploads(&self) {
        let (count, done) = &*self.uploader.pending;
        let mut pending = count.lock().unwrap_or_else(|p| p.into_inner());
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while *pending > 0 {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break;
            }
            pending = done.wait_timeout(pending, left).unwrap_or_else(|p| p.into_inner()).0;
        }
    }

    fn queue_thumb(&self, id: &str, png: Vec<u8>) {
        let mut sender = self.uploader.sender.lock().unwrap_or_else(|p| p.into_inner());
        if sender.is_none() {
            let (tx, rx) = channel::<(String, Vec<u8>)>();
            let weak = self.me.clone();
            let pending = self.uploader.pending.clone();
            std::thread::spawn(move || {
                while let Ok((id, png)) = rx.recv() {
                    if let Some(library) = weak.upgrade() {
                        library.upload_thumb(&id, &png);
                    }
                    let (count, done) = &*pending;
                    let mut count = count.lock().unwrap_or_else(|p| p.into_inner());
                    *count = count.saturating_sub(1);
                    done.notify_all();
                }
            });
            *sender = Some(tx);
        }
        if let Some(tx) = sender.as_ref() {
            *self.uploader.pending.0.lock().unwrap_or_else(|p| p.into_inner()) += 1;
            if tx.send((id.to_string(), png)).is_err() {
                let mut count = self.uploader.pending.0.lock().unwrap_or_else(|p| p.into_inner());
                *count = count.saturating_sub(1);
            }
        }
    }

    // ---- deleting ------------------------------------------------------------------

    /// Delete items on the server, then bring the replica up to date from the answer
    /// (the rows that left; their files and pictures that nothing else uses go with
    /// them) and read the catalog back.
    pub fn delete_assets(&self, ids: &[String]) -> RemoteResult<Catalog> {
        self.delete(ids)?;
        load_catalog_inner(&self.handle())?.ok_or_else(|| RemoteError::Local("The catalog could not be read back from the replica.".into()))
    }

    /// [`RemoteLibrary::delete_assets`] without reading the catalog back.
    pub fn delete(&self, ids: &[String]) -> RemoteResult<()> {
        self.require_write()?;
        let reply = self.client().delete_assets(ids)?;
        self.catch_up(reply.base, reply.changes)
    }

    // ---- library tags and provenance ---------------------------------------------

    /// Change the library tags and provenance of items on the server, then bring
    /// this computer's replica up to date from the answer (only the rows the edit
    /// changed are written, and nothing else is downloaded when the replica was
    /// current). Returns the touched top-level items as this computer now sees
    /// them, personal metadata included.
    pub fn update_metadata(&self, edits: &[MetadataEdit]) -> RemoteResult<Vec<Asset>> {
        self.require_write()?;
        toybox_core::ops::metadata::validate_edits(edits).map_err(RemoteError::from)?;
        let reply = self.client().update_metadata(edits)?;
        self.catch_up(reply.base, reply.changes)?;
        let store = Store::for_handle(&self.handle());
        Ok(store.load_top_assets(&reply.touched)?)
    }

    /// Drop local files and pictures of the `doomed` items that the catalog no longer has.
    fn forget_items(&self, store: &Store, doomed: &[Asset]) {
        let ids: Vec<String> = doomed.iter().flat_map(|asset| std::iter::once(asset.id.clone()).chain(asset.variants.iter().map(|v| v.id.clone()))).collect();
        let still_there: std::collections::HashSet<String> = store.load_assets(&ids).unwrap_or_default().into_iter().map(|asset| asset.id).collect();
        let handle = self.handle();
        let mut candidates: Vec<String> = Vec::new();
        let mut gone_ids: Vec<String> = Vec::new();
        for asset in doomed.iter().filter(|a| !still_there.contains(&a.id)) {
            asset.collect_files(&mut candidates);
            gone_ids.extend(std::iter::once(asset.id.clone()).chain(asset.variants.iter().map(|v| v.id.clone())).filter(|id| !still_there.contains(id)));
        }
        candidates.sort();
        candidates.dedup();
        // A file another item still lists stays.
        let in_use = store.files_in_use(&candidates).unwrap_or_default();
        let gone_files: Vec<String> = candidates.into_iter().filter(|file| !in_use.contains(file)).collect();
        {
            let mut state = self.lock();
            for id in &gone_ids {
                let _ = std::fs::remove_file(handle.thumb_path(id));
                let _ = std::fs::remove_file(handle.thumb_path(id).with_extension("stale"));
                state.saved.local_thumbs.remove(id);
                state.saved.shared_thumbs.remove(id);
            }
        }
        self.cache.remove(&gone_files);
        self.save_state();
    }

    // ---- the file cache --------------------------------------------------------------

    /// Bytes of downloaded files.
    pub fn cache_size(&self) -> u64 {
        self.cache.size()
    }

    /// Delete the downloaded files (the catalog, pictures and your favourites stay).
    pub fn clear_cache(&self) -> u64 {
        self.cache.clear()
    }

    /// The thumbnail ids the server has pictures for (as of the last sync).
    pub fn shared_thumb_count(&self) -> usize {
        self.lock().saved.shared_thumbs.len()
    }
}

impl FileProvider for RemoteLibrary {
    fn ensure_files(&self, rels: &[String]) -> AppResult<()> {
        self.ensure(rels).map_err(AppError::from)
    }

    fn has_shared_thumb(&self, asset_id: &str) -> bool {
        self.lock().saved.shared_thumbs.contains_key(asset_id)
    }

    fn ensure_thumb(&self, asset_id: &str) -> AppResult<bool> {
        let handle = self.handle();
        if handle.thumb_path(asset_id).is_file() {
            return Ok(true);
        }
        let Some(version) = self.lock().saved.shared_thumbs.get(asset_id).cloned() else { return Ok(false) };
        let Some(png) = self.client().thumb(asset_id).map_err(AppError::from)? else { return Ok(false) };
        if not_a_png(&png) {
            return Err(AppError::msg("The server sent a thumbnail that is not a PNG image"));
        }
        fsio::write_bytes_atomic(&handle.thumb_path(asset_id), &png)?;
        self.stats.thumb_downloads.fetch_add(1, Ordering::Relaxed);
        self.lock().saved.local_thumbs.insert(asset_id.to_string(), version);
        Ok(true)
    }

    fn share_thumb(&self, asset_id: &str, png: &[u8]) {
        if self.can_write() && !not_a_png(png) {
            self.queue_thumb(asset_id, png.to_vec());
        }
    }
}

/// The role and server name remembered from a past visit, read without opening the library.
pub(crate) fn read_saved_summary(data_dir: &Path) -> (Option<RemoteRole>, String) {
    let saved = std::fs::read(data_dir.join("remote.json")).ok().and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok()).unwrap_or_default();
    (saved.role, saved.server_name)
}
