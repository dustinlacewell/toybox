//! Kept source files (`origin.files`): the file a material was rendered from, the file a model was
//! converted from. Each distinct file is stored once per library, by content, in
//! `library/<type>/_sources/<sha256>/<original name>`; the entries that keep it point at that path.
//!
//! An importer still stages a bare file name next to the entry's files. The host moves it into this
//! place when the entry is committed (and drops the staged copy when that content is stored already).
//! The original name is the last segment of the path, so a reader that looks a source up by its name
//! (`origin.meta.sbsar`) finds it without knowing where the file sits.

use std::path::Path;

/// The folder inside a type folder that holds the shared sources.
pub const SOURCES_DIR: &str = "_sources";

/// Which type folders keep sources.
pub fn keeps_sources(type_folder: &str) -> bool {
    matches!(type_folder, "materials" | "models")
}

/// `library/<type>/_sources/<hash>/<name>`.
pub fn shared_path(type_folder: &str, sha256: &str, name: &str) -> String {
    format!("library/{type_folder}/{SOURCES_DIR}/{sha256}/{name}")
}

/// The parts of a shared source path: `(type folder, sha256, name)`; `None` when `path` is anything else
/// (a bare name, a file in an item's folder).
pub fn parse_shared(path: &str) -> Option<(&str, &str, &str)> {
    let mut parts = path.split('/');
    let (library, type_folder, dir, hash, name) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let well_formed = parts.next().is_none()
        && library == "library"
        && dir == SOURCES_DIR
        && keeps_sources(type_folder)
        && hash.len() == 64
        && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && !name.is_empty()
        && !name.contains(['\\', ':'])
        && !matches!(name, "." | "..");
    well_formed.then_some((type_folder, hash, name))
}

/// Whether `path` is a shared source path.
pub fn is_shared(path: &str) -> bool {
    parse_shared(path).is_some()
}

/// The `library/<type>/_sources` folder that holds `path`'s siblings, for a shared path.
pub fn sources_dir_of(path: &str) -> Option<String> {
    parse_shared(path).map(|(type_folder, _, _)| format!("library/{type_folder}/{SOURCES_DIR}"))
}

/// Last segment of a path.
pub fn file_name(path: &str) -> &str {
    Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn shared_paths_round_trip_and_nothing_else_parses() {
        let path = shared_path("materials", HASH, "Stone.sbsar");
        assert_eq!(parse_shared(&path), Some(("materials", HASH, "Stone.sbsar")));
        assert_eq!(file_name(&path), "Stone.sbsar");
        assert_eq!(sources_dir_of(&path).unwrap(), "library/materials/_sources");
        for bad in [
            "Stone.sbsar",
            "library/materials/abc/Stone.sbsar",
            "library/audio/_sources/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef/a.wav",
            "library/materials/_sources/XYZ/Stone.sbsar",
            "library/materials/_sources/0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef/a",
            "library/materials/_sources/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef/a/b",
            "library/materials/_sources/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef/..",
        ] {
            assert_eq!(parse_shared(bad), None, "{bad}");
        }
    }
}
