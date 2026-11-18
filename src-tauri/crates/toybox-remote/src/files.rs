//! The local file cache of a remote library: the same library-relative layout
//! as the server, so every path-based reader works on it. Files are fetched on
//! demand into a temporary name and renamed into place once complete and the
//! right size. Storage paths are content addressed, so a cached file never goes
//! stale and nothing is ever evicted except by "Clear downloaded files".
//! Two requests for the same file download it once.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

use crate::client::Client;
use crate::error::{RemoteError, RemoteResult};

/// Folders under `library/` that hold imports being prepared for upload, not downloads.
const STAGING_PREFIX: &str = "_import_";

static PART_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct FileCache {
    root: PathBuf,
    in_flight: Mutex<HashSet<String>>,
    finished: Condvar,
}

/// A library-relative path the cache will store: `library/...` made of plain segments.
pub fn valid_rel(rel: &str) -> bool {
    rel.starts_with("library/")
        && rel.len() <= 1024
        && !rel.contains(['\\', ':'])
        && !rel.chars().any(char::is_control)
        && rel.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
        && rel.split('/').count() >= 3
        && !rel.split('/').nth(1).is_some_and(|folder| folder.starts_with(STAGING_PREFIX))
        && !rel.ends_with(".part")
}

impl FileCache {
    pub fn new(root: PathBuf) -> FileCache {
        FileCache { root, in_flight: Mutex::new(HashSet::new()), finished: Condvar::new() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        let mut path = self.root.clone();
        path.extend(rel.split('/'));
        path
    }

    pub fn has(&self, rel: &str) -> bool {
        self.path(rel).is_file()
    }

    /// Make `rel` exist locally, downloading it unless it is already there or
    /// another thread is fetching it right now (then wait for that one).
    /// Returns the bytes fetched by this call (0 when nothing had to be fetched).
    pub fn ensure(&self, client: &Client, rel: &str) -> RemoteResult<u64> {
        if !valid_rel(rel) {
            return Err(RemoteError::Local(format!("Not a file of a library: {rel}")));
        }
        loop {
            if self.has(rel) {
                return Ok(0);
            }
            let mut flying = self.in_flight.lock().unwrap_or_else(|p| p.into_inner());
            if flying.contains(rel) {
                while flying.contains(rel) {
                    flying = self.finished.wait(flying).unwrap_or_else(|p| p.into_inner());
                }
                continue;
            }
            flying.insert(rel.to_string());
            break;
        }
        let result = self.download(client, rel);
        self.in_flight.lock().unwrap_or_else(|p| p.into_inner()).remove(rel);
        self.finished.notify_all();
        result
    }

    fn download(&self, client: &Client, rel: &str) -> RemoteResult<u64> {
        let target = self.path(rel);
        let parent = target.parent().ok_or_else(|| RemoteError::Local(format!("Not a file of a library: {rel}")))?;
        let mut download = client.file(rel)?;
        std::fs::create_dir_all(parent)?;
        let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let part = parent.join(format!("{name}.{}-{}.part", std::process::id(), PART_COUNTER.fetch_add(1, Ordering::Relaxed)));
        let written = (|| -> RemoteResult<u64> {
            let mut file = std::fs::File::create(&part)?;
            let mut buffer = vec![0u8; 64 * 1024];
            let mut total = 0u64;
            loop {
                let count = download.reader.read(&mut buffer).map_err(|e| RemoteError::Unreachable { server: client.base().to_string(), detail: format!("the download was interrupted: {e}") })?;
                if count == 0 {
                    break;
                }
                file.write_all(&buffer[..count])?;
                total += count as u64;
            }
            file.flush()?;
            if let Some(expected) = download.len {
                if expected != total {
                    return Err(RemoteError::Unreachable { server: client.base().to_string(), detail: format!("the download of {name} was cut short ({total} of {expected} bytes)") });
                }
            }
            Ok(total)
        })();
        match written {
            Ok(total) => {
                if std::fs::rename(&part, &target).is_err() {
                    // Someone else finished the same file first: theirs is as good as ours.
                    let exists = target.is_file();
                    let _ = std::fs::remove_file(&part);
                    if !exists {
                        return Err(RemoteError::Local(format!("Could not save the downloaded file {rel} on this computer.")));
                    }
                }
                Ok(total)
            }
            Err(error) => {
                let _ = std::fs::remove_file(&part);
                Err(error)
            }
        }
    }

    /// Delete cached files, and the folders they leave empty.
    pub fn remove(&self, rels: &[String]) {
        for rel in rels {
            if !valid_rel(rel) {
                continue;
            }
            let path = self.path(rel);
            if std::fs::remove_file(&path).is_ok() {
                let mut folder = path.parent().map(Path::to_path_buf);
                while let Some(dir) = folder {
                    if dir == self.root || !dir.starts_with(&self.root) || std::fs::remove_dir(&dir).is_err() {
                        break;
                    }
                    folder = dir.parent().map(Path::to_path_buf);
                }
            }
        }
    }

    fn downloads(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.root.join("library")) else { return Vec::new() };
        entries
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with(STAGING_PREFIX))
            .map(|entry| entry.path())
            .collect()
    }

    /// Bytes of downloaded files (imports being staged are not counted).
    pub fn size(&self) -> u64 {
        fn walk(path: &Path) -> u64 {
            match std::fs::symlink_metadata(path) {
                Ok(meta) if meta.is_dir() => std::fs::read_dir(path).map(|entries| entries.flatten().map(|e| walk(&e.path())).sum()).unwrap_or(0),
                Ok(meta) if meta.is_file() => meta.len(),
                _ => 0,
            }
        }
        self.downloads().iter().map(|path| walk(path)).sum()
    }

    /// Delete every downloaded file (not imports being staged). Returns the bytes freed.
    pub fn clear(&self) -> u64 {
        let freed = self.size();
        for path in self.downloads() {
            let _ = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        }
        freed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_library_files_are_cacheable() {
        assert!(valid_rel("library/models/0123456789abcdef/a.gltf"));
        assert!(valid_rel("library/models/_resources/abc.png"));
        for bad in ["", "library", "library/x", "_library_config/catalog.json", "library/_import_ab/x/y.png", "library/a/../b/c", "library//a/b", "library/a/b/", "/library/a/b", "library\\a\\b", "library/a/b:c/d", "library/a/b/c.part", "C:/library/a/b"] {
            assert!(!valid_rel(bad), "{bad}");
        }
    }
}
