//! Thin filesystem helpers. The only module besides command glue that touches
//! `std::fs` for reading/writing app files. Keeps error mapping in one place.

use std::path::Path;

use crate::error::{AppResult,AppError};

pub fn read_text(path: &Path) -> AppResult<String> {
    std::fs::read_to_string(path).map_err(|error|AppError::file("Read",path,error))
}

pub fn read_bytes(path: &Path) -> AppResult<Vec<u8>> {
    std::fs::read(path).map_err(|error|AppError::file("Read",path,error))
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error|AppError::file("Create directory",parent,error))?;
    }
    std::fs::write(path, bytes).map_err(|error|AppError::file("Write",path,error))?;
    Ok(())
}

pub fn write_text(path: &Path, text: &str) -> AppResult<()> {
    write_bytes(path, text.as_bytes())
}

/// Publish a complete replacement without truncating a file the UI is reading.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let temporary = path.with_extension("pending");
    write_bytes(&temporary, bytes)?;
    let result = std::fs::rename(&temporary, path).map_err(|error| AppError::file("Replace", path, error));
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result
}

pub fn exists(path: &Path) -> bool {
    path.exists()
}

/// Atomic deletion attempt: missing is already the desired cache state.
pub fn remove_file_if_exists(path:&Path)->AppResult<()> {
    for attempt in 0..4 {
        match std::fs::remove_file(path) {
            Ok(())=>return Ok(()),
            Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(()),
            // Concurrent Windows deletions briefly report ACCESS_DENIED while
            // the first unlink is pending, before the name disappears.
            Err(error) if cfg!(windows)&&attempt<3&&matches!(error.raw_os_error(),Some(5|32))=>std::thread::sleep(std::time::Duration::from_millis(1)),
            Err(error)=>return Err(AppError::file("Remove",path,error)),
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thumbnail_replacement_publishes_complete_bytes() {
        let root=std::env::temp_dir().join(format!("toybox-thumb-replace-{}",std::process::id()));
        let path=root.join("thumb.png");
        write_bytes(&path,b"old thumbnail").unwrap();
        write_bytes_atomic(&path,b"complete new thumbnail").unwrap();
        assert_eq!(read_bytes(&path).unwrap(),b"complete new thumbnail");
        assert!(!path.with_extension("pending").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn concurrent_cache_invalidation_is_idempotent_and_missing_reads_name_the_file() {
        let root=std::env::temp_dir().join(format!("toybox-fsio-race-{}",std::process::id()));
        std::fs::create_dir_all(&root).unwrap();let path=root.join("thumb.png");std::fs::write(&path,b"png").unwrap();
        let barrier=std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads:Vec<_>=(0..8).map(|_|{let path=path.clone();let barrier=barrier.clone();std::thread::spawn(move||{barrier.wait();remove_file_if_exists(&path)})}).collect();
        for thread in threads {thread.join().unwrap().unwrap();}
        remove_file_if_exists(&path).unwrap();
        assert!(read_text(&path).unwrap_err().to_string().contains("thumb.png"));
        std::fs::remove_dir(root).unwrap();
    }
}
