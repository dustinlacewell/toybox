//! Safe resolution of library-relative paths that come from outside the process
//! (a URL, an upload target). A caller hands in text like `library/audio/ab/x.wav`
//! and gets back a filesystem path that is guaranteed to stay inside the library
//! root: no `..`, no absolute paths, no Windows separators or drive letters, and
//! no symbolic link that leads out of the root.
//!
//! The check has two layers. `clean_rel` is purely textual and rejects anything
//! that is not a plain relative path. `resolve_existing` / `resolve_for_write`
//! then look at the disk and canonicalise, so a link inside the library that
//! points elsewhere cannot be used to escape.

use std::fmt;
use std::path::{Path, PathBuf};

/// Why a path was refused. The messages are plain language and safe to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelPathError {
    Empty,
    TooLong,
    Absolute,
    Traversal,
    BadCharacter,
    NotFound,
    NotAFile,
    Escapes,
}

impl fmt::Display for RelPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RelPathError::Empty => "the path is empty",
            RelPathError::TooLong => "the path is too long",
            RelPathError::Absolute => "the path must be relative to the library",
            RelPathError::Traversal => "the path may not contain '.' or '..' parts",
            RelPathError::BadCharacter => "the path contains a character or name that is not allowed (or that Windows cannot store)",
            RelPathError::NotFound => "no such file",
            RelPathError::NotAFile => "that is not a file",
            RelPathError::Escapes => "the path leaves the library",
        })
    }
}

impl std::error::Error for RelPathError {}

const MAX_PATH: usize = 1024;
const MAX_SEGMENT: usize = 255;

/// Validate and normalise a library-relative path. Only `/` separates parts;
/// a backslash is refused rather than converted, so `a\..\b` cannot hide a
/// traversal from one platform's rules. Returns the path with single `/`
/// separators and no leading or trailing slash.
pub fn clean_rel(rel: &str) -> Result<String, RelPathError> {
    if rel.is_empty() {
        return Err(RelPathError::Empty);
    }
    if rel.len() > MAX_PATH {
        return Err(RelPathError::TooLong);
    }
    if rel.starts_with('/') {
        return Err(RelPathError::Absolute);
    }
    let mut parts = Vec::new();
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(RelPathError::Traversal);
        }
        if part.len() > MAX_SEGMENT {
            return Err(RelPathError::TooLong);
        }
        // `:` covers drive letters (`C:`), NTFS alternate streams and URL-like
        // text; a trailing dot or space is silently dropped by Windows, which
        // would turn `..  ` or `a. ` into something else.
        if part.chars().any(|c| c == '\\' || c == ':' || c == '\0' || c.is_control())
            || part.ends_with('.')
            || part.ends_with(' ')
        {
            return Err(RelPathError::BadCharacter);
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

/// Whether a name that is legal here (on Linux almost anything is) would be
/// refused or changed by Windows: the characters `< > " | ? *`, and the device
/// names `CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9` with or
/// without an extension. A library is copied between the two systems, so names
/// that cannot be copied down to a Windows machine are refused on the way in.
pub fn windows_unsafe_name(part: &str) -> bool {
    if part.chars().any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*')) {
        return true;
    }
    let stem = part.split('.').next().unwrap_or("").trim_end_matches(' ').to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

/// `clean_rel`, plus the names that cannot travel to Windows.
pub fn portable_rel(rel: &str) -> Result<String, RelPathError> {
    let rel = clean_rel(rel)?;
    if rel.split('/').any(windows_unsafe_name) {
        return Err(RelPathError::BadCharacter);
    }
    Ok(rel)
}

/// Whether two library-relative paths would be the same file on a
/// case-insensitive file system (Windows, and macOS by default) without being
/// the same on this one: the first part where they differ differs only in case.
pub fn case_clash(a: &str, b: &str) -> bool {
    for (x, y) in a.split('/').zip(b.split('/')) {
        if x != y {
            return x.to_lowercase() == y.to_lowercase();
        }
    }
    false
}

fn canonical_root(root: &Path) -> Result<PathBuf, RelPathError> {
    root.canonicalize().map_err(|_| RelPathError::NotFound)
}

/// Resolve an existing regular file under `root`. The returned path is
/// canonical and inside the canonical root, even when links are involved.
pub fn resolve_existing(root: &Path, rel: &str) -> Result<PathBuf, RelPathError> {
    let rel = clean_rel(rel)?;
    let root = canonical_root(root)?;
    let mut joined = root.clone();
    joined.extend(rel.split('/'));
    let real = joined.canonicalize().map_err(|_| RelPathError::NotFound)?;
    if !real.starts_with(&root) {
        return Err(RelPathError::Escapes);
    }
    if !real.is_file() {
        return Err(RelPathError::NotAFile);
    }
    Ok(real)
}

/// Resolve a path that is about to be created (or replaced) under `root`.
/// Nothing is created. The deepest ancestor that already exists must be a real
/// directory inside the root, and the target itself must not be a link.
pub fn resolve_for_write(root: &Path, rel: &str) -> Result<PathBuf, RelPathError> {
    let rel = clean_rel(rel)?;
    let root = canonical_root(root)?;
    let mut joined = root.clone();
    joined.extend(rel.split('/'));
    // Walk up to the first ancestor that exists on disk. `symlink_metadata`
    // sees a dangling link that `exists()` would call missing.
    let mut probe = joined.as_path();
    loop {
        match std::fs::symlink_metadata(probe) {
            Ok(_) => break,
            // Linux answers `ENOTDIR` (not "not found") for a path below a regular file;
            // Windows answers "not found". Both mean: keep walking up to the file.
            Err(error) if matches!(error.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) => {
                probe = probe.parent().ok_or(RelPathError::Escapes)?;
            }
            Err(_) => return Err(RelPathError::Escapes),
        }
    }
    let real = probe.canonicalize().map_err(|_| RelPathError::Escapes)?;
    if !real.starts_with(&root) {
        return Err(RelPathError::Escapes);
    }
    if probe == joined {
        // The target exists already: it must be the file itself, not a link
        // or a directory, since a write would go through or fail.
        let meta = std::fs::symlink_metadata(&joined).map_err(|_| RelPathError::Escapes)?;
        if meta.file_type().is_symlink() {
            return Err(RelPathError::Escapes);
        }
        if meta.is_dir() {
            return Err(RelPathError::NotAFile);
        }
    } else if !real.is_dir() {
        return Err(RelPathError::NotAFile);
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Tmp {
            let dir = std::env::temp_dir().join(format!("toybox-files-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("root/library/audio/ab")).unwrap();
            std::fs::write(dir.join("root/library/audio/ab/x.wav"), b"wav").unwrap();
            std::fs::write(dir.join("secret.txt"), b"secret").unwrap();
            Tmp(dir)
        }
        fn root(&self) -> PathBuf {
            self.0.join("root")
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn clean_accepts_plain_relative_paths() {
        assert_eq!(clean_rel("library/audio/ab/x.wav").unwrap(), "library/audio/ab/x.wav");
        assert_eq!(clean_rel("a b/c d.png").unwrap(), "a b/c d.png");
        assert_eq!(clean_rel("a/.hidden").unwrap(), "a/.hidden");
    }

    #[test]
    fn clean_rejects_traversal_absolute_and_windows_forms() {
        let bad = [
            "", "/etc/passwd", "../secret.txt", "library/../../secret.txt", "library/./x", "library//x",
            "library/x/", "a/..", "..", ".", "C:/Windows/win.ini", "C:\\Windows\\win.ini", "library\\audio\\x.wav",
            "..\\secret.txt", "library/x\\..\\..\\secret.txt", "\\\\server\\share\\x", "library/a:b", "file.txt:stream",
            "library/nul\0.txt", "library/line\nbreak", "trailing./x", "space /x", "library/..%2f", "x/ ..",
        ];
        for path in bad {
            // `..%2f` is a literal name here (decoding is the caller's job) and is allowed;
            // the rest must be refused.
            if path == "library/..%2f" {
                assert!(clean_rel(path).is_ok());
                continue;
            }
            assert!(clean_rel(path).is_err(), "{path:?} should be refused");
        }
        assert_eq!(clean_rel(&"a/".repeat(600)).unwrap_err(), RelPathError::TooLong);
        assert_eq!(clean_rel(&format!("{}/x", "n".repeat(300))).unwrap_err(), RelPathError::TooLong);
    }

    #[test]
    fn names_that_cannot_travel_to_windows_are_recognised() {
        for bad in ["a|b.png", "what?.png", "x*.bin", "<a>.txt", "q\".txt", "CON", "con.txt", "Nul.tar.gz", "aux", "COM1.png", "lpt9", "com3 .txt"] {
            assert!(windows_unsafe_name(bad), "{bad}");
            assert_eq!(portable_rel(&format!("library/{bad}")).unwrap_err(), RelPathError::BadCharacter, "{bad}");
        }
        for good in ["console.png", "com10.png", "COM.png", "lpt.png", "a b.png", ".hidden", "auxiliary", "nulled.bin", "COM0"] {
            assert!(!windows_unsafe_name(good), "{good}");
            assert!(portable_rel(&format!("library/{good}")).is_ok(), "{good}");
        }
        // Everything `clean_rel` refuses stays refused.
        assert_eq!(portable_rel("library/../x").unwrap_err(), RelPathError::Traversal);
    }

    #[test]
    fn case_clashes_are_paths_that_differ_only_in_case() {
        assert!(case_clash("library/a/Tex.png", "library/a/tex.png"));
        assert!(case_clash("library/Skybox/px.png", "library/skybox/nx.png"), "the folder differs only in case");
        assert!(!case_clash("library/a/tex.png", "library/a/tex.png"), "the same path is not a clash");
        assert!(!case_clash("library/a/tex.png", "library/a/tex2.png"));
        assert!(!case_clash("library/a/b", "library/a/b/c"));
    }

    #[test]
    fn resolves_files_inside_the_root_only() {
        let tmp = Tmp::new("resolve");
        let found = resolve_existing(&tmp.root(), "library/audio/ab/x.wav").unwrap();
        assert!(found.is_file() && found.starts_with(tmp.root().canonicalize().unwrap()));
        assert_eq!(resolve_existing(&tmp.root(), "library/audio/ab/missing.wav").unwrap_err(), RelPathError::NotFound);
        assert_eq!(resolve_existing(&tmp.root(), "library/audio").unwrap_err(), RelPathError::NotAFile);
        assert!(resolve_existing(&tmp.root(), "../secret.txt").is_err());
        let absolute = tmp.0.join("secret.txt");
        assert!(resolve_existing(&tmp.root(), &absolute.to_string_lossy()).is_err());
        assert!(resolve_existing(&tmp.root(), &absolute.to_string_lossy().replace('\\', "/")).is_err());
    }

    #[test]
    fn write_targets_must_stay_inside_and_are_not_created() {
        let tmp = Tmp::new("write");
        let target = resolve_for_write(&tmp.root(), "library/_import_a/new/dir/file.bin").unwrap();
        assert!(target.starts_with(tmp.root().canonicalize().unwrap()));
        assert!(!tmp.root().join("library/_import_a").exists(), "nothing is created");
        // Replacing an existing file is fine; replacing a directory is not.
        assert!(resolve_for_write(&tmp.root(), "library/audio/ab/x.wav").is_ok());
        assert_eq!(resolve_for_write(&tmp.root(), "library/audio").unwrap_err(), RelPathError::NotAFile);
        // A file where a directory is needed.
        assert_eq!(resolve_for_write(&tmp.root(), "library/audio/ab/x.wav/inner.bin").unwrap_err(), RelPathError::NotAFile);
        assert_eq!(resolve_for_write(&tmp.root(), "library/audio/ab/x.wav/a/b/inner.bin").unwrap_err(), RelPathError::NotAFile);
        assert!(resolve_for_write(&tmp.root(), "../escape.bin").is_err());
        assert!(resolve_for_write(&tmp.root(), "a\\..\\..\\escape.bin").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_lead_out_of_the_root() {
        use std::os::unix::fs::symlink;
        let tmp = Tmp::new("links-unix");
        symlink(tmp.0.join("secret.txt"), tmp.root().join("library/audio/ab/link.txt")).unwrap();
        symlink(&tmp.0, tmp.root().join("library/outside")).unwrap();
        symlink(tmp.root().join("library/audio/ab/x.wav"), tmp.root().join("library/audio/ab/inner.wav")).unwrap();
        assert_eq!(resolve_existing(&tmp.root(), "library/audio/ab/link.txt").unwrap_err(), RelPathError::Escapes);
        assert_eq!(resolve_existing(&tmp.root(), "library/outside/secret.txt").unwrap_err(), RelPathError::Escapes);
        // A link that stays inside the root is harmless and resolves to the real file.
        assert!(resolve_existing(&tmp.root(), "library/audio/ab/inner.wav").is_ok());
        // Writes may not follow a link out, nor replace a link.
        assert!(resolve_for_write(&tmp.root(), "library/outside/new.bin").is_err());
        assert!(resolve_for_write(&tmp.root(), "library/audio/ab/link.txt").is_err());
        // A dangling link is not "missing": it is refused.
        symlink(tmp.0.join("does-not-exist"), tmp.root().join("library/dangling")).unwrap();
        assert!(resolve_for_write(&tmp.root(), "library/dangling/x.bin").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn symlinks_cannot_lead_out_of_the_root_windows() {
        use std::os::windows::fs::{symlink_dir, symlink_file};
        let tmp = Tmp::new("links-win");
        // Creating links needs a privilege; without it there is nothing to test.
        if symlink_file(tmp.0.join("secret.txt"), tmp.root().join("library/audio/ab/link.txt")).is_err() {
            return;
        }
        symlink_dir(&tmp.0, tmp.root().join("library/outside")).unwrap();
        assert_eq!(resolve_existing(&tmp.root(), "library/audio/ab/link.txt").unwrap_err(), RelPathError::Escapes);
        assert_eq!(resolve_existing(&tmp.root(), "library/outside/secret.txt").unwrap_err(), RelPathError::Escapes);
        assert!(resolve_for_write(&tmp.root(), "library/outside/new.bin").is_err());
    }
}
