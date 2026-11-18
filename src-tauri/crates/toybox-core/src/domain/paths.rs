//! Pure path transforms between Godot `res://` paths, library-relative paths,
//! and the relative URIs found inside glTF files. No filesystem access.

/// Strip Godot's `res://assets/` prefix, yielding a path relative to the
/// library root. `res://assets/library/polygon_city/buildings`
/// -> `library/polygon_city/buildings`.
pub fn res_to_rel(res_path: &str) -> String {
    res_path
        .trim_start_matches("res://assets/")
        .trim_start_matches("res://")
        .to_string()
}

/// Split `library/<pack>/<category>` into `(pack, category)`.
/// Returns `None` if the shape doesn't match.
pub fn pack_category_from_rel_dir(rel_dir: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = rel_dir.split('/').collect();
    match parts.as_slice() {
        ["library", pack, category, ..] => Some((pack.to_string(), category.to_string())),
        _ => None,
    }
}

/// Resolve a relative glTF URI (e.g. `../textures/Foo.png` or `Foo.bin`)
/// against the directory containing the glTF, normalizing `.`/`..` segments.
/// `gltf_rel_dir` is library-relative (e.g. `library/polygon_city/buildings`).
/// Returns a library-relative path (e.g. `library/polygon_city/textures/Foo.png`).
pub fn resolve_uri_rel(gltf_rel_dir: &str, uri: &str) -> String {
    let mut stack: Vec<&str> = gltf_rel_dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in uri.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    stack.join("/")
}

/// Join a library root with a library-relative path into an absolute path,
/// using the OS separator.
pub fn abs_under_root(library_root: &str, rel: &str) -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(library_root);
    for seg in rel.split('/').filter(|s| !s.is_empty()) {
        p.push(seg);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_res_prefix() {
        assert_eq!(
            res_to_rel("res://assets/library/polygon_city/buildings"),
            "library/polygon_city/buildings"
        );
    }

    #[test]
    fn splits_pack_category() {
        assert_eq!(
            pack_category_from_rel_dir("library/polygon_city/buildings"),
            Some(("polygon_city".into(), "buildings".into()))
        );
    }

    #[test]
    fn resolves_parent_uri() {
        assert_eq!(
            resolve_uri_rel("library/polygon_city/buildings", "../textures/Foo.png"),
            "library/polygon_city/textures/Foo.png"
        );
    }

    #[test]
    fn resolves_sibling_uri() {
        assert_eq!(
            resolve_uri_rel("library/polygon_city/buildings", "Bar.bin"),
            "library/polygon_city/buildings/Bar.bin"
        );
    }
}
