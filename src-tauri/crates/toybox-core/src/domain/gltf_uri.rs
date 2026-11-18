//! What a glTF `uri` (of a buffer or an image) refers to. It is one of:
//!
//! * a **file** next to the glTF (`Crate.bin`, `../_resources/ab12.png`,
//!   `My%20Tex.png`), the only kind that is part of a library's file set;
//! * an **embedded** `data:` URI that carries the bytes inline (FBX2glTF does
//!   this for images that are not backed by a file). It is not a file: it has no
//!   path, nothing to copy, hash, own or delete;
//! * a **remote** reference (`http://...`), which a library never owns either.
//!
//! Every place that turns a `uri` into a path goes through this module, so none
//! of them can mistake an embedded image for a relative file name.

use crate::domain::paths;
use crate::error::{AppError, AppResult};

/// Whether `uri` is an embedded `data:` URI.
pub fn is_data_uri(uri: &str) -> bool {
    uri.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
}

/// Whether `uri` starts with a URI scheme other than `data:` and a Windows drive
/// letter (`http:`, `https:`, `file:`, `toybox-resource:` ...).
pub fn is_remote_uri(uri: &str) -> bool {
    let Some((scheme, _)) = uri.split_once(':') else { return false };
    scheme.len() >= 2
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// The reference when it names a file (relative or not: absolute and escaping
/// names are for the caller to reject), `None` for embedded and remote ones.
pub fn external_file(uri: &str) -> Option<&str> {
    (!is_data_uri(uri) && !is_remote_uri(uri)).then_some(uri)
}

/// Whether a library-relative path recorded in a catalog is really an embedded
/// URI that an older version resolved against the glTF folder
/// (`library/models/<key>/data:image/png;base64,...`). Such records are not files.
pub fn is_embedded_path(rel: &str) -> bool {
    rel.split(['/', '\\']).any(is_data_uri)
}

/// The library-relative file a glTF at folder `parent_rel` names with `uri`.
///
/// `Ok(None)` for embedded and remote references. Absolute, drive-lettered and
/// backslashed names are refused, so a scene can only name library media; a
/// `..` that climbs out of the library is caught by the callers' root checks.
pub fn resolve_library_file(parent_rel: &str, uri: &str) -> AppResult<Option<String>> {
    let Some(uri) = external_file(uri) else { return Ok(None) };
    if uri.contains(':') || uri.starts_with('/') || uri.contains('\\') {
        return Err(AppError::msg("External scene URI is not library media"));
    }
    Ok(Some(paths::resolve_uri_rel(parent_rel, &decode_percent(uri)?)))
}

/// Undo `%XX` escapes in a file reference.
pub fn decode_percent(uri: &str) -> AppResult<String> {
    let mut bytes = Vec::with_capacity(uri.len());
    let mut chars = uri.as_bytes().iter().copied();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let a = chars.next().and_then(|b| (b as char).to_digit(16));
            let b = chars.next().and_then(|b| (b as char).to_digit(16));
            match (a, b) {
                (Some(a), Some(b)) => bytes.push((a * 16 + b) as u8),
                _ => return Err(AppError::msg("Invalid scene URI escape")),
            }
        } else {
            bytes.push(byte)
        }
    }
    String::from_utf8(bytes).map_err(|_| AppError::msg("Invalid UTF-8 scene URI"))
}

/// The MIME type and bytes of a `data:` URI (base64 or percent-encoded), or
/// `None` when it is not one or is malformed.
pub fn decode_data_uri(uri: &str) -> Option<(String, Vec<u8>)> {
    if !is_data_uri(uri) {
        return None;
    }
    let (header, payload) = uri[5..].split_once(',')?;
    let mut parts = header.split(';');
    let mime = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let base64 = parts.any(|part| part.trim().eq_ignore_ascii_case("base64"));
    let bytes = if base64 { decode_base64(payload)? } else { decode_percent(payload).ok()?.into_bytes() };
    Some((if mime.is_empty() { "text/plain".into() } else { mime }, bytes))
}

/// File extension (without dot) for an image MIME type, for naming.
pub fn image_extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/ktx2" => "ktx2",
        _ => "bin",
    }
}

fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut accumulator, mut bits, mut padding) = (0u32, 0u32, 0usize);
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => {
                padding += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        if padding > 0 {
            return None;
        }
        accumulator = (accumulator << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
            accumulator &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIXEL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7w==";

    #[test]
    fn data_and_remote_references_are_not_files() {
        assert!(is_data_uri(PIXEL));
        assert!(is_data_uri("DATA:text/plain,hi"));
        assert_eq!(external_file(PIXEL), None);
        assert_eq!(external_file("https://example.com/a.png"), None);
        assert_eq!(external_file("http://example.com/a.png"), None);
        assert_eq!(external_file("file:///c/a.png"), None);
        assert_eq!(external_file("Crate.bin"), Some("Crate.bin"));
        assert_eq!(external_file("../textures/a b.png"), Some("../textures/a b.png"));
        // Absolute and drive-lettered names stay "files" so callers keep rejecting them.
        assert_eq!(external_file("C:\\a.png"), Some("C:\\a.png"));
        assert_eq!(external_file("/etc/a.png"), Some("/etc/a.png"));
        assert_eq!(external_file("..\\a.png"), Some("..\\a.png"));
    }

    #[test]
    fn resolves_files_and_refuses_unsafe_names() {
        assert_eq!(resolve_library_file("library/models/k", "a.bin").unwrap().as_deref(), Some("library/models/k/a.bin"));
        assert_eq!(resolve_library_file("library/models/k", "../_resources/ab.png").unwrap().as_deref(), Some("library/models/_resources/ab.png"));
        assert_eq!(resolve_library_file("library/models/k", "My%20Tex%2B.png").unwrap().as_deref(), Some("library/models/k/My Tex+.png"));
        assert_eq!(resolve_library_file("library/models/k", PIXEL).unwrap(), None);
        assert_eq!(resolve_library_file("library/models/k", "https://x/y.png").unwrap(), None);
        for bad in ["/abs.png", "C:\\abs.png", "..\\up.png", "a\\b.png"] {
            assert!(resolve_library_file("library/models/k", bad).is_err(), "{bad}");
        }
        assert!(resolve_library_file("library/models/k", "bad%zz.png").is_err());
    }

    #[test]
    fn old_catalog_records_of_embedded_images_are_recognised() {
        assert!(is_embedded_path(&format!("library/models/abc/{PIXEL}")));
        assert!(is_embedded_path("library/models/abc/data:image/png;base64,AAAA"));
        assert!(!is_embedded_path("library/models/_resources/abc.png"));
        assert!(!is_embedded_path("library/models/data/abc.png"));
    }

    #[test]
    fn data_uris_decode() {
        let (mime, bytes) = decode_data_uri(PIXEL).unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(decode_data_uri("data:application/octet-stream;base64,AAECAw==").unwrap().1, vec![0, 1, 2, 3]);
        assert_eq!(decode_data_uri("data:text/plain,a%20b").unwrap().1, b"a b");
        assert!(decode_data_uri("data:image/png;base64,@@@@").is_none());
        assert!(decode_data_uri("data:nocomma").is_none());
        assert!(decode_data_uri("Crate.bin").is_none());
        assert_eq!(image_extension("image/jpeg"), "jpg");
    }
}
