//! Thin filesystem helpers. The only module besides command glue that touches
//! `std::fs` for reading/writing app files. Keeps error mapping in one place.

use std::path::Path;

use crate::error::AppResult;

pub fn read_text(path: &Path) -> AppResult<String> {
    Ok(std::fs::read_to_string(path)?)
}

pub fn read_bytes(path: &Path) -> AppResult<Vec<u8>> {
    Ok(std::fs::read(path)?)
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

pub fn write_text(path: &Path, text: &str) -> AppResult<()> {
    write_bytes(path, text.as_bytes())
}

pub fn exists(path: &Path) -> bool {
    path.exists()
}
