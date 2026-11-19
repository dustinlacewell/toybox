//! The jailed filesystem surface plugins call through `ctx.fs`.
//!
//! Filesystem boundary (the real one, JS capability flags are advisory):
//! - [`Host::plugin_read_text`] is jailed to the plugins folder (the loader reads
//!   plugin source through it; a plugin can't read arbitrary files by spoofing a path).
//! - `write_*` accept an `authorized_root` (the user-picked target dir or save-file
//!   the current run is scoped to) and reject any path that escapes it.
//! - Reads and listings are jailed to the `source_root` the user picked.
//! - The `plugin://` protocol handler (in the app) is jailed to the plugins folder the same way.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use toybox_core::domain::image_embed;
use toybox_core::error::{AppError, AppResult};
use toybox_core::infra::fsio;

use crate::host::Host;

// --- jailed filesystem surface --------------------------------------------

/// One entry of a plugin's source-directory walk. `rel_path` is relative to the
/// `source_root` the read was authorized against (forward-slashed), so a plugin
/// recurses by feeding it straight back as the next `path`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryDto {
    pub name: String,
    pub rel_path: String,
    pub is_dir: bool,
}

/// List one directory level under a user-authorized `source_root` (the folder
/// the user picked to import from), jailed to it the same lexical way writes are,
/// with source-root-relative `rel_path`s. Importers use this to walk a source tree
/// they don't own.
pub fn read_dir_under(source_root: &Path, path: &str) -> AppResult<Vec<DirEntryDto>> {
    let abs = jail(source_root, path)?;
    let base = path.trim_end_matches(['/', '\\']);
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&abs)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        // `entry.path().is_dir()` (not `file_type()`) so directory junctions —
        // how dev tooling links folders on Windows — are followed, matching
        // `list_plugins`.
        let is_dir = entry.path().is_dir();
        let rel_path = if base.is_empty() || base == "." {
            name.clone()
        } else {
            format!("{base}/{name}")
        };
        out.push(DirEntryDto { name, rel_path, is_dir });
    }
    Ok(out)
}

/// Read a file's bytes under a user-authorized `source_root`. Same jail as
/// [`read_dir_under`]; importers use it to pull source asset bytes for copy/unpack.
pub fn read_bytes_under(source_root: &Path, path: &str) -> AppResult<Vec<u8>> {
    let abs = jail(source_root, path)?;
    fsio::read_bytes(&abs)
}

/// Copy one file from a user-authorized `source_root` to a path under
/// `authorized_root`. Both ends are jailed exactly like [`read_bytes_under`] and
/// [`write_bytes_under`]. An existing destination is only accepted when it already
/// holds the same bytes.
pub fn copy_file_under(source_root: &Path, path: &str, authorized_root: &Path, target: &str) -> AppResult<()> {
    let from = jail(source_root, path)?;
    let to = jail(authorized_root, target)?;
    copy_file(&from, &to)
}

fn copy_file(from: &Path, to: &Path) -> AppResult<()> {
    if !from.is_file() {
        return Err(AppError::msg(format!("not a file: {}", from.display())));
    }
    if to.exists() {
        if fsio::read_bytes(to)? == fsio::read_bytes(from)? {
            return Ok(());
        }
        return Err(AppError::msg(format!("a different file already exists at {}", to.display())));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to)?;
    Ok(())
}

/// Read an image under `source_root` as PNG bytes scaled down to fit
/// `max_size` x `max_size`. Same jail as [`read_bytes_under`].
pub fn image_thumb_under(source_root: &Path, path: &str, max_size: u32) -> AppResult<Vec<u8>> {
    let abs = jail(source_root, path)?;
    let format = image::ImageFormat::from_path(&abs).ok();
    image_embed::thumb_png(&fsio::read_bytes(&abs)?, format, max_size)
}

/// Write bytes under the run's `authorized_root` (the user-picked target). Any
/// `path` that escapes `authorized_root` is rejected.
pub fn write_bytes_under(authorized_root: &Path, path: &str, bytes: &[u8]) -> AppResult<()> {
    let abs = jail(authorized_root, path)?;
    fsio::write_bytes(&abs, bytes)
}

/// Write text under the run's `authorized_root`, same jail as [`write_bytes_under`].
pub fn write_text_under(authorized_root: &Path, path: &str, text: &str) -> AppResult<()> {
    let abs = jail(authorized_root, path)?;
    fsio::write_text(&abs, text)
}

/// Remove a file or folder below `authorized_root`. The root itself is never removed, and a path
/// that does not exist is not an error (plugins use this to tidy up after themselves).
pub fn remove_tree_under(authorized_root: &Path, path: &str) -> AppResult<()> {
    let abs = jail(authorized_root, path)?;
    if normalize(&abs) == normalize(authorized_root) {
        return Err(AppError::msg("the root of a jail cannot be removed"));
    }
    match std::fs::symlink_metadata(&abs) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&abs).map_err(AppError::from),
        Ok(_) => std::fs::remove_file(&abs).map_err(AppError::from),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

impl Host {
    /// Read a UTF-8 file the plugin loader needs, jailed to the plugins folder.
    pub fn plugin_read_text(&self, path: &str) -> AppResult<String> {
        let root = self.paths.plugins_dir();
        let abs = jail(&root, path)?;
        fsio::read_text(&abs)
    }

    /// Existence check, jailed to the plugins folder.
    pub fn plugin_exists(&self, path: &str) -> bool {
        let root = self.paths.plugins_dir();
        // exists() must not error for a missing file; only reject jail escapes.
        jail(&root, path).map(|p| fsio::exists(&p)).unwrap_or(false)
    }
}

/// Resolve `path` under `root` and confirm it does not escape. `path` may be
/// absolute (must then be inside `root`) or relative (joined onto `root`). The
/// check is lexical — no component may be `..` and an absolute path must share
/// `root`'s prefix — so it holds even for not-yet-existing files (canonicalize
/// would fail on those).
pub fn jail(root: &Path, path: &str) -> AppResult<PathBuf> {
    let candidate = Path::new(path);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };

    if joined.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(AppError::msg(format!("path escapes its jail: {path}")));
    }
    let root_norm = normalize(root);
    let joined_norm = normalize(&joined);
    if !joined_norm.starts_with(&root_norm) {
        return Err(AppError::msg(format!(
            "path '{path}' is outside the authorized root"
        )));
    }
    Ok(joined)
}

/// Lexical normalization: collapse `.` segments and compare case-insensitively
/// on Windows by lowercasing. No filesystem access (works for missing files).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::Normal(s) => {
                #[cfg(windows)]
                out.push(s.to_string_lossy().to_lowercase());
                #[cfg(not(windows))]
                out.push(s);
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_tree_stays_inside_its_jail_and_ignores_what_is_not_there() {
        let dir = std::env::temp_dir().join(format!("toybox-remove-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("scratch/render-1/deep")).unwrap();
        std::fs::write(dir.join("scratch/render-1/deep/a.png"), b"x").unwrap();
        std::fs::write(dir.join("scratch/render-2.txt"), b"y").unwrap();
        std::fs::write(dir.join("outside.txt"), b"z").unwrap();
        let root = dir.join("scratch");
        remove_tree_under(&root, "render-1").unwrap();
        remove_tree_under(&root, "render-2.txt").unwrap();
        remove_tree_under(&root, "never-there").unwrap();
        assert!(!root.join("render-1").exists() && !root.join("render-2.txt").exists());
        assert!(remove_tree_under(&root, "../outside.txt").is_err());
        assert!(remove_tree_under(&root, ".").is_err(), "the root of the jail stays");
        assert!(dir.join("outside.txt").exists() && root.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_files_once_and_refuses_to_replace_different_content() {
        let dir=std::env::temp_dir().join(format!("toybox-copy-test-{}",std::process::id()));
        let _=std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("source")).unwrap();
        std::fs::write(dir.join("source/sky.hdr"),b"light").unwrap();
        let from=jail(&dir.join("source"),"sky.hdr").unwrap();
        let to=jail(&dir.join("library"),"library/_import_x/skybox/Sky/sky.hdr").unwrap();
        copy_file(&from,&to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(),b"light");
        // Repeating an interrupted import is harmless; a clash is not silent.
        copy_file(&from,&to).unwrap();
        std::fs::write(dir.join("source/sky.hdr"),b"other").unwrap();
        assert!(copy_file(&from,&to).is_err());
        assert_eq!(std::fs::read(&to).unwrap(),b"light");
        assert!(copy_file(&dir.join("source"),&dir.join("library/folder")).is_err());
        assert!(jail(&dir.join("library"),"../source/sky.hdr").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn jail_allows_paths_under_root() {
        let root = Path::new(r"C:\proj\out");
        assert!(jail(root, "sub/file.glb").is_ok());
        assert!(jail(root, r"C:\proj\out\sub\file.glb").is_ok());
    }

    #[test]
    fn jail_rejects_parent_escape() {
        let root = Path::new(r"C:\proj\out");
        assert!(jail(root, "../evil.txt").is_err());
        assert!(jail(root, "sub/../../evil.txt").is_err());
    }

    #[test]
    fn jail_rejects_absolute_outside_root() {
        let root = Path::new(r"C:\proj\out");
        assert!(jail(root, r"C:\windows\system32\evil.dll").is_err());
    }

    #[test]
    fn read_dir_walks_with_recursable_rel_paths() {
        // A unique temp source tree, removed at both ends so a prior failure
        // can't poison the run.
        let src = std::env::temp_dir().join("toybox_plugins_read_dir_test");
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(src.join("pack")).unwrap();
        std::fs::write(src.join("top.gltf"), "{}").unwrap();
        std::fs::write(src.join("pack").join("a.gltf"), "{}").unwrap();

        // Top level: rel_path is name-only for "." so JS can recurse from it.
        let top = read_dir_under(&src, ".").unwrap();
        let pack = top.iter().find(|e| e.name == "pack").expect("pack dir");
        assert!(pack.is_dir);
        assert_eq!(pack.rel_path, "pack");

        // Recurse using the returned rel_path verbatim; nested rel_path is
        // source-root-relative and forward-slashed.
        let nested = read_dir_under(&src, &pack.rel_path).unwrap();
        let a = nested.iter().find(|e| e.name == "a.gltf").expect("a.gltf");
        assert_eq!(a.rel_path, "pack/a.gltf");
        assert!(!a.is_dir);

        // The rel_path round-trips through the jail into a byte read.
        let abs = jail(&src, &a.rel_path).unwrap();
        assert_eq!(fsio::read_bytes(&abs).unwrap(), b"{}");

        // A parent escape is rejected for reads too.
        assert!(jail(&src, "../escape").is_err());

        let _ = std::fs::remove_dir_all(&src);
    }
}
