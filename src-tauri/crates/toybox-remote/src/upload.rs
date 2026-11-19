//! Writing to a remote library: importing staged files, and uploading a
//! transfer package. Both go through the server's upload area (`POST /uploads`,
//! streamed `PUT`s, then a commit or an ingest) and finish by bringing the local
//! catalog up to date.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde_json::Value;
use toybox_core::domain::catalog_model::Catalog;
use toybox_core::domain::{seed, sources};
use toybox_core::ops::transfer::{ItemFailure, OnConflict, Outcome, ReadFile, TransferItem, TransferPackage, TransferProgress, TransferReport};

use crate::error::{RemoteError, RemoteResult};
use crate::library::RemoteLibrary;
use crate::client::Client;
use crate::types::{CommitReply, EntryError, UploadLink};

/// What an import did.
#[derive(Debug)]
pub struct ImportOutcome {
    pub imported: Vec<String>,
    /// Entries the server refused, with the reason for each.
    pub errors: Vec<EntryError>,
    /// The library's catalog after the import.
    pub catalog: Catalog,
}

struct Staged {
    rel: String,
    path: PathBuf,
    size: u64,
}

fn walk(root: &Path, folder: &Path, out: &mut Vec<Staged>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            walk(root, &path, out);
        } else if kind.is_file() && path.extension().is_none_or(|ext| ext != "part") {
            let rel = path.strip_prefix(root).map(|r| r.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"));
            if let (Ok(rel), Ok(meta)) = (rel, entry.metadata()) {
                out.push(Staged { rel, path, size: meta.len() });
            }
        }
    }
}

/// The staging packs (`_import_...`) the entries and their variants name.
fn staging_packs(entries: &[Value]) -> RemoteResult<BTreeSet<String>> {
    let mut packs = BTreeSet::new();
    for entry in entries {
        for item in std::iter::once(entry).chain(entry["variants"].as_array().into_iter().flatten()) {
            match item["pack"].as_str() {
                Some(pack) if pack.starts_with("_import_") && !pack.contains(['/', '\\']) => {
                    packs.insert(pack.to_string());
                }
                _ => return Err(RemoteError::Local("An import entry does not name its staging folder (a pack like _import_<name>), so its files cannot be found.".into())),
            }
        }
    }
    Ok(packs)
}

impl RemoteLibrary {
    /// Import into this library what an importer staged under the file cache
    /// (`library/_import_<name>/...`) and described with `entries`: upload the
    /// files, commit the entries, remove the local staging and refresh the
    /// catalog. Entries the server refuses are returned in `errors`; the others
    /// are in the library.
    pub fn import(&self, entries: Vec<Value>) -> RemoteResult<ImportOutcome> {
        let (imported, errors) = self.import_rows(entries)?;
        let catalog = toybox_core::ops::catalog::load_catalog_inner(&self.handle())?
            .ok_or_else(|| RemoteError::Local("The catalog could not be read back from the replica.".into()))?;
        Ok(ImportOutcome { imported, errors, catalog })
    }

    /// [`RemoteLibrary::import`] without reading the catalog back: the ids that were added and the
    /// entries the server refused.
    pub fn import_rows(&self, entries: Vec<Value>) -> RemoteResult<(Vec<String>, Vec<EntryError>)> {
        self.require_write()?;
        let packs = staging_packs(&entries)?;
        let root = self.cache.root().to_path_buf();
        let result = self.import_staged(&root, &packs, &entries);
        // The staging is only a hand-over: a failed import is started again from the importer.
        for pack in &packs {
            let _ = std::fs::remove_dir_all(root.join("library").join(pack));
        }
        let reply = result?;
        // The answer holds the rows the import changed; they go into the replica without a download.
        self.catch_up(reply.base, reply.changes)?;
        Ok((reply.imported, reply.errors))
    }

    fn import_staged(&self, root: &Path, packs: &BTreeSet<String>, entries: &[Value]) -> RemoteResult<CommitReply> {
        let mut files = Vec::new();
        for pack in packs {
            walk(root, &root.join("library").join(pack), &mut files);
        }
        let client = self.client();
        // A kept source (a material's archive) is sent once, and not at all when the server stores it.
        let links = plan_staged_source_links(&client, entries, &mut files)?;
        let upload = client.create_upload()?;
        let total_files = files.len() as u64;
        let total_bytes: u64 = files.iter().map(|f| f.size).sum();
        let outcome = (|| -> RemoteResult<_> {
            let workers = worker_count(files.len());
            let pool = Pool::new(workers);
            let mut last_files = 0u64;
            let mut last_bytes = 0u64;
            let mut tick = |pool: &Pool| {
                let done = pool.done.load(Ordering::Relaxed);
                let sent = pool.sent.load(Ordering::Relaxed);
                // Report about every 256 KB so a big file moves the bar without flooding the window.
                if done != last_files || sent - last_bytes >= 256 * 1024 {
                    last_files = done;
                    last_bytes = sent;
                    self.emit_progress("upload", done, total_files, sent, total_bytes, pool.label());
                }
            };
            if !files.is_empty() {
                std::thread::scope(|scope| {
                    for _ in 0..workers {
                        scope.spawn(|| pool.work(&client, &upload.id, Route::Staged));
                    }
                    for staged in &files {
                        let job = Job { rel: staged.rel.clone(), size: staged.size, body: Body::File(staged.path.clone()) };
                        if !pool.push(job, &mut tick) {
                            break;
                        }
                    }
                    pool.close();
                    pool.wait_idle(&mut tick);
                });
            }
            pool.finish()?;
            if !links.is_empty() {
                client.link_upload(&upload.id, &links)?;
            }
            client.commit(&upload.id, entries)
        })();
        match outcome {
            Ok(reply) => {
                if reply.upload_kept {
                    let _ = client.cancel_upload(&upload.id);
                }
                Ok(reply)
            }
            Err(error) => {
                if error.is_connectivity() {
                    self.go_offline(&error);
                }
                cancel_after_failure(&client, &upload.id);
                Err(error)
            }
        }
    }

    /// Upload a transfer package: stream every file it lists (read through
    /// `read`), then have the server ingest it with the same code a local copy
    /// runs. `progress` counts items, in proportion to the bytes sent.
    pub fn upload_package(&self, package: &TransferPackage, on_conflict: OnConflict, read: ReadFile, progress: &dyn Fn(TransferProgress)) -> RemoteResult<TransferReport> {
        self.require_write()?;
        let total_items = package.assets.len() + package.failed.len();
        let mut sendable = package.clone();
        // Favourites and tags stay on this computer; the server has none.
        for asset in &mut sendable.assets {
            asset.user.clear();
        }
        sendable.source_library = String::new();
        if sendable.assets.is_empty() {
            return Ok(failed_only(&sendable.failed));
        }
        let mut files: Vec<(String, u64)> = Vec::new();
        let mut seen_paths: HashSet<&str> = HashSet::new();
        for asset in &sendable.assets {
            for file in asset.files.iter().chain(asset.thumbs.iter().map(|t| &t.file)) {
                if seen_paths.insert(file.path.as_str()) {
                    files.push((file.path.clone(), file.size));
                }
            }
        }
        // A kept source the server stores already is not sent: the server links it into the upload.
        let client = self.client();
        let links = plan_package_source_links(&client, &sendable, &mut files)?;
        let total_bytes: u64 = files.iter().map(|(_, size)| size).sum();
        let upload = client.create_upload()?;
        progress(TransferProgress { done: package.failed.len(), total: total_items });
        let outcome = (|| -> RemoteResult<_> {
            let workers = worker_count(files.len());
            let pool = Pool::new(workers);
            let total_files = files.len() as u64;
            let mut last_files = 0u64;
            let mut last_bytes = 0u64;
            let mut last_sent = 0u64;
            // Only ever called on this thread: `progress` and `read` are not thread-safe.
            let mut tick = |pool: &Pool| {
                let sent = pool.sent.load(Ordering::Relaxed);
                if sent != last_sent {
                    last_sent = sent;
                    let items = if total_bytes == 0 { 0 } else { (sendable.assets.len() as u64 * sent / total_bytes) as usize };
                    progress(TransferProgress { done: (package.failed.len() + items).min(total_items.saturating_sub(1)), total: total_items });
                }
                let done = pool.done.load(Ordering::Relaxed);
                if done != last_files || sent - last_bytes >= 256 * 1024 {
                    last_files = done;
                    last_bytes = sent;
                    self.emit_progress("upload", done, total_files, sent, total_bytes, pool.label());
                }
            };
            std::thread::scope(|scope| {
                for _ in 0..workers {
                    scope.spawn(|| pool.work(&client, &upload.id, Route::Package));
                }
                for (path, size) in &files {
                    if pool.stopped() {
                        break;
                    }
                    // A file that cannot be read at its source fails its items on the server's side, not the whole transfer.
                    let Ok(mut source) = read(path) else { continue };
                    if *size <= BUFFER_LIMIT {
                        // Small files are read here (the reader may not leave this thread) and sent by a worker.
                        let mut bytes = Vec::with_capacity(*size as usize);
                        if source.read_to_end(&mut bytes).is_err() {
                            continue;
                        }
                        let job = Job { rel: path.clone(), size: bytes.len() as u64, body: Body::Bytes(bytes) };
                        if !pool.push(job, &mut tick) {
                            break;
                        }
                    } else {
                        // A big file is streamed from here, while the workers carry on with the queue.
                        pool.send_direct(&client, &upload.id, Route::Package, path, &mut source, *size, &mut tick);
                    }
                }
                pool.close();
                pool.wait_idle(&mut tick);
            });
            pool.finish()?;
            if !links.is_empty() {
                client.link_upload(&upload.id, &links)?;
            }
            client.ingest(&upload.id, &sendable, on_conflict)
        })();
        match outcome {
            Ok(reply) => {
                progress(TransferProgress { done: total_items, total: total_items });
                // The items are on the server whatever happens here; a replica that cannot be updated
                // now is brought up to date by the next sync.
                if let Err(error) = self.catch_up(reply.base, reply.changes) {
                    eprintln!("toybox: could not update the replica of {} after the transfer: {error}", self.id());
                }
                Ok(reply.report)
            }
            Err(error) => {
                if error.is_connectivity() {
                    self.go_offline(&error);
                }
                cancel_after_failure(&client, &upload.id);
                Err(error)
            }
        }
    }
}

/// The SHA-256 of a file, streamed.
fn hash_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            return Ok(format!("{:x}", hash.finalize()));
        }
        hash.update(&buffer[..count]);
    }
}

/// Take the staged kept sources of `entries` out of `files` when they need not be sent: those the server
/// stores (linked by content) and the copies of one content after the first (linked from it). Returns the
/// links to ask the server for. A server that cannot say which sources it stores gets every file.
fn plan_staged_source_links(client: &Client, entries: &[Value], files: &mut Vec<Staged>) -> RemoteResult<Vec<UploadLink>> {
    let mut staged: HashSet<String> = HashSet::new();
    for entry in entries {
        if let Ok(parsed) = seed::parse_entry(entry) {
            for item in std::iter::once(&parsed).chain(&parsed.variants) {
                staged.extend(item.origin.iter().flat_map(|origin| origin.files.iter().cloned()).filter(|path| !sources::is_shared(path)));
            }
        }
    }
    if staged.is_empty() {
        return Ok(Vec::new());
    }
    let mut by_hash: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files.iter().filter(|file| staged.contains(&file.rel)) {
        if let Ok(hash) = hash_file(&file.path) {
            by_hash.entry(hash).or_default().push(file.rel.clone());
        }
    }
    if by_hash.is_empty() {
        return Ok(Vec::new());
    }
    let hashes: Vec<String> = by_hash.keys().cloned().collect();
    let Some(have) = client.sources_have(&hashes)? else { return Ok(Vec::new()) };
    let mut links = Vec::new();
    for (hash, rels) in by_hash {
        let first_sent = !have.contains(&hash);
        for (index, rel) in rels.iter().enumerate() {
            if first_sent && index == 0 {
                continue;
            }
            links.push(if first_sent {
                UploadLink { path: rel.clone(), route: "files".into(), sha256: None, from: Some(rels[0].clone()) }
            } else {
                UploadLink { path: rel.clone(), route: "files".into(), sha256: Some(hash.clone()), from: None }
            });
        }
    }
    let linked: HashSet<&str> = links.iter().map(|link| link.path.as_str()).collect();
    files.retain(|file| !linked.contains(file.rel.as_str()));
    Ok(links)
}

/// The same for a transfer package: its kept sources (shared paths, whose digests the package states) that
/// the server stores are linked instead of sent.
fn plan_package_source_links(client: &Client, package: &TransferPackage, files: &mut Vec<(String, u64)>) -> RemoteResult<Vec<UploadLink>> {
    let mut digests: BTreeMap<String, String> = BTreeMap::new();
    for asset in &package.assets {
        for file in &asset.files {
            if sources::is_shared(&file.path) {
                digests.insert(file.path.clone(), file.sha256.clone());
            }
        }
    }
    if digests.is_empty() {
        return Ok(Vec::new());
    }
    let hashes: Vec<String> = digests.values().cloned().collect::<BTreeSet<_>>().into_iter().collect();
    let Some(have) = client.sources_have(&hashes)? else { return Ok(Vec::new()) };
    let mut links = Vec::new();
    for (path, hash) in &digests {
        if have.contains(hash) {
            links.push(UploadLink { path: path.clone(), route: "package".into(), sha256: Some(hash.clone()), from: None });
        }
    }
    let linked: HashSet<&str> = links.iter().map(|link| link.path.as_str()).collect();
    files.retain(|(path, _)| !linked.contains(path.as_str()));
    Ok(links)
}

/// Files up to this size are read into memory so a worker thread can send them.
const BUFFER_LIMIT: u64 = 8 * 1024 * 1024;
/// How many files are on the wire at once.
const MAX_WORKERS: usize = 6;

fn worker_count(files: usize) -> usize {
    files.clamp(1, MAX_WORKERS)
}

#[derive(Clone, Copy)]
enum Route {
    /// `PUT /uploads/{id}/files/...`
    Staged,
    /// `PUT /uploads/{id}/package/...`
    Package,
}

enum Body {
    File(PathBuf),
    Bytes(Vec<u8>),
}

struct Job {
    rel: String,
    size: u64,
    body: Body,
}

struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

/// A fixed set of worker threads sending files from a shared queue. The thread
/// that fills the queue also reports progress (it is the only one allowed to),
/// reading the counters the workers keep.
struct Pool {
    queue: Mutex<Queue>,
    wake: Condvar,
    capacity: usize,
    running: AtomicUsize,
    stop: AtomicBool,
    /// Bytes handed to the network so far, over all files.
    sent: AtomicU64,
    /// Files completely sent.
    done: AtomicU64,
    error: Mutex<Option<RemoteError>>,
    label: Mutex<Option<String>>,
}

/// Counts what passes through and gives up as soon as the pool is stopping.
struct Watch<'a, R> {
    inner: R,
    pool: &'a Pool,
    tick: Option<&'a mut dyn FnMut(&Pool)>,
}

impl<R: Read> Read for Watch<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pool.stop.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "the upload was stopped"));
        }
        let count = self.inner.read(buf)?;
        if count > 0 {
            self.pool.sent.fetch_add(count as u64, Ordering::Relaxed);
            if let Some(tick) = self.tick.as_mut() {
                tick(self.pool);
            }
        }
        Ok(count)
    }
}

impl Pool {
    fn new(workers: usize) -> Pool {
        Pool {
            queue: Mutex::new(Queue { jobs: VecDeque::new(), closed: false }),
            wake: Condvar::new(),
            capacity: workers + 2,
            running: AtomicUsize::new(workers),
            stop: AtomicBool::new(false),
            sent: AtomicU64::new(0),
            done: AtomicU64::new(0),
            error: Mutex::new(None),
            label: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn label(&self) -> Option<String> {
        self.label.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Keep the first error and stop everyone.
    fn fail(&self, error: RemoteError) {
        self.error.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert(error);
        self.stop.store(true, Ordering::Relaxed);
        self.wake.notify_all();
    }

    fn finish(&self) -> RemoteResult<()> {
        match self.error.lock().unwrap_or_else(|p| p.into_inner()).take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Queue a job, waiting while the queue is full. False when the pool has stopped.
    fn push(&self, job: Job, tick: &mut dyn FnMut(&Pool)) -> bool {
        let mut queue = self.lock();
        loop {
            if self.stopped() {
                return false;
            }
            if queue.jobs.len() < self.capacity {
                queue.jobs.push_back(job);
                drop(queue);
                self.wake.notify_all();
                return true;
            }
            queue = self.wake.wait_timeout(queue, Duration::from_millis(100)).unwrap_or_else(|p| p.into_inner()).0;
            drop(queue);
            tick(self);
            queue = self.lock();
        }
    }

    fn close(&self) {
        self.lock().closed = true;
        self.wake.notify_all();
    }

    /// Wait for the workers to finish, reporting progress meanwhile.
    fn wait_idle(&self, tick: &mut dyn FnMut(&Pool)) {
        while self.running.load(Ordering::Acquire) > 0 {
            let queue = self.lock();
            drop(self.wake.wait_timeout(queue, Duration::from_millis(100)).unwrap_or_else(|p| p.into_inner()).0);
            tick(self);
        }
        tick(self);
    }

    fn put(&self, client: &Client, upload: &str, route: Route, rel: &str, reader: &mut dyn Read, size: u64) -> RemoteResult<()> {
        match route {
            Route::Staged => client.put_upload_file(upload, rel, reader, size),
            Route::Package => client.put_package_file(upload, rel, reader, size),
        }
    }

    fn completed(&self, rel: &str) {
        self.done.fetch_add(1, Ordering::Relaxed);
        *self.label.lock().unwrap_or_else(|p| p.into_inner()) = Some(rel.to_string());
        self.wake.notify_all();
    }

    /// Stream one file from the calling thread.
    #[allow(clippy::too_many_arguments)]
    fn send_direct(&self, client: &Client, upload: &str, route: Route, rel: &str, source: &mut dyn Read, size: u64, tick: &mut dyn FnMut(&Pool)) {
        let mut reader = Watch { inner: source, pool: self, tick: Some(tick) };
        match self.put(client, upload, route, rel, &mut reader, size) {
            Ok(()) => self.completed(rel),
            Err(error) => self.fail(error),
        }
    }

    fn work(&self, client: &Client, upload: &str, route: Route) {
        loop {
            let job = {
                let mut queue = self.lock();
                loop {
                    if self.stopped() {
                        break None;
                    }
                    if let Some(job) = queue.jobs.pop_front() {
                        break Some(job);
                    }
                    if queue.closed {
                        break None;
                    }
                    queue = self.wake.wait(queue).unwrap_or_else(|p| p.into_inner());
                }
            };
            let Some(job) = job else { break };
            self.wake.notify_all();
            let result = match &job.body {
                Body::Bytes(bytes) => {
                    let mut reader = Watch { inner: bytes.as_slice(), pool: self, tick: None };
                    self.put(client, upload, route, &job.rel, &mut reader, job.size)
                }
                Body::File(path) => match std::fs::File::open(path) {
                    Ok(file) => {
                        let mut reader = Watch { inner: file, pool: self, tick: None };
                        self.put(client, upload, route, &job.rel, &mut reader, job.size)
                    }
                    Err(error) => Err(error.into()),
                },
            };
            match result {
                Ok(()) => self.completed(&job.rel),
                Err(error) => {
                    self.fail(error);
                    break;
                }
            }
        }
        self.running.fetch_sub(1, Ordering::AcqRel);
        self.wake.notify_all();
    }
}

/// Remove the upload from the server after a failure. A write the server has not
/// yet noticed was abandoned (we hung up mid-file) makes it answer "busy" for a
/// moment, so a few more tries are made.
fn cancel_after_failure(client: &Client, upload: &str) {
    for attempt in 0..8 {
        match client.cancel_upload(upload) {
            Err(RemoteError::Conflict { .. }) if attempt < 7 => std::thread::sleep(Duration::from_millis(100)),
            _ => return,
        }
    }
}

fn failed_only(failed: &[ItemFailure]) -> TransferReport {
    TransferReport {
        items: failed
            .iter()
            .map(|failure| TransferItem {
                id: failure.id.clone(),
                name: failure.name.clone(),
                kind: failure.kind.clone(),
                outcome: Outcome::Failed,
                reason: Some(failure.reason.clone()),
                source_removed: false,
            })
            .collect(),
    }
}

