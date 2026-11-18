//! The staging area for imports in progress. A client creates an upload, PUTs
//! files into it, then commits entries that refer to those files. Nothing in
//! here touches the library: files sit under `<data>/uploads/<id>/files/`
//! until `library::commit` places them for core to publish.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::random_hex;
use crate::error::ApiError;

#[derive(Debug, Clone, Copy)]
pub struct UploadLimits {
    pub max_file_bytes: u64,
    pub max_upload_bytes: u64,
    pub max_files: usize,
    pub max_uploads: usize,
    /// An upload nobody has touched for this long is removed.
    pub idle_timeout: Duration,
}

impl Default for UploadLimits {
    fn default() -> Self {
        UploadLimits {
            max_file_bytes: 2048 * 1024 * 1024,
            max_upload_bytes: 8192 * 1024 * 1024,
            max_files: 5000,
            max_uploads: 16,
            idle_timeout: Duration::from_secs(60 * 60),
        }
    }
}

struct Upload {
    touched: Instant,
    /// Bytes counted against the upload: finished files plus reserved bytes of
    /// files still being written.
    bytes: u64,
    files: HashMap<String, u64>,
    /// Every folder or file name claimed by a file (finished, or being written),
    /// keyed by its exact parent path and its lower-cased name, with the spelling
    /// used and how many files claim it. This answers "does another file differ
    /// from this one only in case" without scanning the files.
    names: HashMap<String, (String, u32)>,
    writers: u32,
    committing: bool,
}

pub struct Uploads {
    dir: PathBuf,
    pub limits: UploadLimits,
    inner: Mutex<HashMap<String, Upload>>,
}

/// Held while a file is being written; gives its reservation back on drop
/// unless `finish` was called.
pub struct FileSlot<'a> {
    uploads: &'a Uploads,
    id: String,
    rel: String,
    reserved: u64,
    /// Size of the file this one replaces, which stops counting once the new one lands.
    replaces: u64,
    /// This slot holds a claim on the names of its path (released on drop, or
    /// on finish when another write of the same path has already landed).
    claimed: bool,
    done: bool,
}

/// One key per component of `rel`: the exact path above it plus its lower-cased
/// name. Two paths clash (see `case_clash`) when they share a key but spell the
/// component differently.
fn name_keys(rel: &str) -> impl Iterator<Item = (String, &str)> {
    let mut prefix_end = 0usize;
    rel.split('/').map(move |part| {
        let key = format!("{}\u{1}{}", &rel[..prefix_end], part.to_lowercase());
        prefix_end += part.len() + 1;
        (key, part)
    })
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Uploads {
    pub fn new(dir: PathBuf, limits: UploadLimits) -> Uploads {
        Uploads { dir, limits, inner: Mutex::new(HashMap::new()) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Upload>> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Remove every leftover staging folder. Uploads do not survive a restart.
    pub fn wipe_all(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        self.lock().clear();
    }

    pub fn create(&self) -> Result<String, ApiError> {
        let mut uploads = self.lock();
        if uploads.len() >= self.limits.max_uploads {
            return Err(ApiError::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "too_many_uploads",
                "Too many imports are in progress. Finish or cancel one and try again.",
            ));
        }
        let id = random_hex(16);
        std::fs::create_dir_all(self.files_dir(&id)).map_err(ApiError::internal)?;
        uploads.insert(id.clone(), Upload { touched: Instant::now(), bytes: 0, files: HashMap::new(), names: HashMap::new(), writers: 0, committing: false });
        Ok(id)
    }

    /// The folder holding the upload's files, laid out like the library root.
    pub fn files_dir(&self, id: &str) -> PathBuf {
        self.dir.join(id).join("files")
    }

    pub fn exists(&self, id: &str) -> bool {
        valid_id(id) && self.lock().contains_key(id)
    }

    fn unknown() -> ApiError {
        ApiError::not_found("There is no such import in progress. It may have expired; start a new one.")
    }

    /// Start writing one file. Reserves nothing yet; see `FileSlot::reserve`.
    pub fn begin_file(&self, id: &str, rel: &str) -> Result<FileSlot<'_>, ApiError> {
        if !valid_id(id) {
            return Err(Self::unknown());
        }
        let mut uploads = self.lock();
        let upload = uploads.get_mut(id).ok_or_else(Self::unknown)?;
        if upload.committing {
            return Err(ApiError::conflict("upload_busy", "This import is being committed. Wait for it to finish."));
        }
        if !upload.files.contains_key(rel) && upload.files.len() >= self.limits.max_files {
            return Err(ApiError::too_large(format!("An import can hold at most {} files.", self.limits.max_files)));
        }
        // Two names that differ only in case are two files here but one on
        // Windows, where this library will be copied: refuse the second.
        if name_keys(rel).any(|(key, part)| upload.names.get(&key).is_some_and(|(spelling, _)| spelling != part)) {
            return Err(ApiError::conflict("path_conflict", "Another file in this import has the same name apart from upper and lower case."));
        }
        let replaces = upload.files.get(rel).copied().unwrap_or(0);
        // The first write of a path claims its names; a re-PUT of a finished path already holds them.
        let claimed = !upload.files.contains_key(rel);
        if claimed {
            for (key, part) in name_keys(rel) {
                upload.names.entry(key).or_insert_with(|| (part.to_string(), 0)).1 += 1;
            }
        }
        upload.writers += 1;
        upload.touched = Instant::now();
        Ok(FileSlot { uploads: self, id: id.to_string(), rel: rel.to_string(), reserved: 0, replaces, claimed, done: false })
    }

    /// Mark an upload as being committed. Fails when files are still arriving
    /// or a commit is already running.
    pub fn begin_commit(&self, id: &str) -> Result<(), ApiError> {
        if !valid_id(id) {
            return Err(Self::unknown());
        }
        let mut uploads = self.lock();
        let upload = uploads.get_mut(id).ok_or_else(Self::unknown)?;
        if upload.committing || upload.writers > 0 {
            return Err(ApiError::conflict("upload_busy", "Files are still being sent, or the import is already being committed."));
        }
        upload.committing = true;
        upload.touched = Instant::now();
        Ok(())
    }

    pub fn end_commit(&self, id: &str) {
        if let Some(upload) = self.lock().get_mut(id) {
            upload.committing = false;
            upload.touched = Instant::now();
        }
    }

    /// Remove an upload and its files. Refused while a commit or a write is running.
    pub fn remove(&self, id: &str) -> Result<(), ApiError> {
        if !valid_id(id) {
            return Err(Self::unknown());
        }
        {
            let mut uploads = self.lock();
            let upload = uploads.get(id).ok_or_else(Self::unknown)?;
            if upload.committing || upload.writers > 0 {
                return Err(ApiError::conflict("upload_busy", "Files are still being sent, or the import is being committed."));
            }
            uploads.remove(id);
        }
        let _ = std::fs::remove_dir_all(self.dir.join(id));
        Ok(())
    }

    /// Remove uploads nobody has touched for the idle timeout. Returns how many.
    pub fn cleanup(&self) -> usize {
        let expired: Vec<String> = {
            let mut uploads = self.lock();
            let ids: Vec<String> = uploads
                .iter()
                .filter(|(_, u)| u.writers == 0 && !u.committing && u.touched.elapsed() > self.limits.idle_timeout)
                .map(|(id, _)| id.clone())
                .collect();
            for id in &ids {
                uploads.remove(id);
            }
            ids
        };
        for id in &expired {
            let _ = std::fs::remove_dir_all(self.dir.join(id));
        }
        expired.len()
    }

    pub fn count(&self) -> usize {
        self.lock().len()
    }
}

impl FileSlot<'_> {
    /// Count `more` bytes of this file against the limits.
    pub fn reserve(&mut self, more: u64) -> Result<(), ApiError> {
        let limits = self.uploads.limits;
        if self.reserved + more > limits.max_file_bytes {
            return Err(ApiError::too_large(format!(
                "That file is larger than the {} MB limit for one file.",
                limits.max_file_bytes / (1024 * 1024)
            )));
        }
        let mut uploads = self.uploads.lock();
        let upload = uploads.get_mut(&self.id).ok_or_else(Uploads::unknown)?;
        // The file being replaced does not count against the new one.
        if upload.bytes - self.replaces.min(upload.bytes) + more > limits.max_upload_bytes {
            return Err(ApiError::too_large(format!(
                "This import is larger than the {} MB limit for one import.",
                limits.max_upload_bytes / (1024 * 1024)
            )));
        }
        upload.bytes += more;
        upload.touched = Instant::now();
        self.reserved += more;
        Ok(())
    }

    /// The file is complete on disk. A previous version of the same path stops counting.
    pub fn finish(mut self) {
        self.done = true;
        let mut uploads = self.uploads.lock();
        if let Some(upload) = uploads.get_mut(&self.id) {
            if let Some(old) = upload.files.insert(self.rel.clone(), self.reserved) {
                upload.bytes = upload.bytes.saturating_sub(old);
                // Another write of this path landed first and holds the claim already.
                if self.claimed {
                    release_names(upload, &self.rel);
                }
            }
            upload.writers = upload.writers.saturating_sub(1);
            upload.touched = Instant::now();
        }
    }
}

impl Drop for FileSlot<'_> {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let mut uploads = self.uploads.lock();
        if let Some(upload) = uploads.get_mut(&self.id) {
            upload.bytes = upload.bytes.saturating_sub(self.reserved);
            upload.writers = upload.writers.saturating_sub(1);
            if self.claimed {
                release_names(upload, &self.rel);
            }
        }
    }
}

fn release_names(upload: &mut Upload, rel: &str) {
    for (key, _) in name_keys(rel) {
        if let Some(entry) = upload.names.get_mut(&key) {
            entry.1 = entry.1.saturating_sub(1);
            if entry.1 == 0 {
                upload.names.remove(&key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uploads(limits: UploadLimits) -> (Uploads, PathBuf) {
        let dir = std::env::temp_dir().join(format!("toybox-uploads-{}-{}", std::process::id(), random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        (Uploads::new(dir.clone(), limits), dir)
    }

    #[test]
    fn limits_are_enforced_and_released() {
        let limits = UploadLimits { max_file_bytes: 100, max_upload_bytes: 150, max_files: 2, max_uploads: 1, ..Default::default() };
        let (uploads, dir) = uploads(limits);
        let id = uploads.create().unwrap();
        assert!(uploads.create().is_err(), "only one upload at a time here");
        let mut a = uploads.begin_file(&id, "a").unwrap();
        a.reserve(100).unwrap();
        assert!(a.reserve(1).is_err(), "per-file limit");
        a.finish();
        let mut b = uploads.begin_file(&id, "b").unwrap();
        b.reserve(50).unwrap();
        assert!(b.reserve(1).is_err(), "per-upload limit");
        drop(b); // an aborted write gives its bytes back
        let mut b = uploads.begin_file(&id, "b").unwrap();
        b.reserve(50).unwrap();
        b.finish();
        assert!(uploads.begin_file(&id, "c").is_err(), "file count limit");
        assert!(uploads.begin_file(&id, "a").is_ok(), "replacing an existing file is fine");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn case_clashes_are_refused_the_way_core_defines_them() {
        let (uploads, dir) = uploads(UploadLimits::default());
        let id = uploads.create().unwrap();
        uploads.begin_file(&id, "library/a/Tex.png").unwrap().finish();
        let paths = [
            "library/a/tex.png", "library/A/tex.png", "library/a/Tex.png", "library/a/tex2.png", "library/a/Tex.png/x", "library/b/Tex.png",
        ];
        for path in paths {
            let expected = toybox_core::ops::files::case_clash("library/a/Tex.png", path);
            assert_eq!(uploads.begin_file(&id, path).is_err(), expected, "{path}");
        }
        // A name claimed by a write still in flight counts too, and is free again once it is dropped.
        let slot = uploads.begin_file(&id, "library/c/Pic.png").unwrap();
        assert!(uploads.begin_file(&id, "library/c/pic.png").is_err());
        drop(slot);
        assert!(uploads.begin_file(&id, "library/c/pic.png").is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn commit_blocks_writers_and_cleanup_removes_idle_uploads() {
        let limits = UploadLimits { idle_timeout: Duration::from_millis(0), ..Default::default() };
        let (uploads, dir) = uploads(limits);
        let id = uploads.create().unwrap();
        uploads.begin_commit(&id).unwrap();
        assert!(uploads.begin_file(&id, "a").is_err());
        assert!(uploads.remove(&id).is_err());
        assert_eq!(uploads.cleanup(), 0, "never while committing");
        uploads.end_commit(&id);
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(uploads.cleanup(), 1);
        assert!(!dir.join(&id).exists());
        assert!(uploads.remove(&id).is_err());
        assert!(!valid_id("../x") && valid_id(&"a".repeat(32)));
        let _ = std::fs::remove_dir_all(dir);
    }
}
