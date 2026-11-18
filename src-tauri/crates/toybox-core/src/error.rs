//! The single error type crossing the command boundary into JS.

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{operation} '{}': {source}", short_path(path))]
    FileIo { operation: &'static str, path: std::path::PathBuf, #[source] source: std::io::Error },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// No library configured yet. Serialized as a stable sentinel so the
    /// frontend can route this to the picker instead of the error screen.
    #[error("no library configured")]
    NoLibrary,
    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn file(operation:&'static str,path:&std::path::Path,source:std::io::Error)->Self {
        Self::FileIo {operation,path:path.to_path_buf(),source}
    }
    pub fn msg(s: impl Into<String>) -> Self {
        AppError::Other(s.into())
    }
}

/// Longest run of a path component shown in a message.
const MAX_COMPONENT: usize = 48;

/// A path for a message: any one component longer than [`MAX_COMPONENT`]
/// characters (an embedded `data:` URI mistaken for a name, say) keeps its start
/// and end around an ellipsis, so a message never carries a blob.
pub fn short_path(path: &std::path::Path) -> String {
    let mut text = path.display().to_string();
    // An embedded URI swallows the rest of the path (its MIME type and base64
    // alphabet contain separators), so cut it as one piece.
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices(['/', '\\']).map(|(index, _)| index + 1))
        .collect();
    if let Some(&start) = starts.iter().find(|&&start| {
        text.get(start..start + 5).is_some_and(|head| head.eq_ignore_ascii_case("data:"))
    }) {
        let keep: String = text[start..].chars().take(24).collect();
        if keep.len() < text.len() - start {
            text = format!("{}{keep}...", &text[..start]);
        }
    }
    let mut out = String::with_capacity(text.len().min(256));
    let mut component = String::new();
    let flush = |component: &mut String, out: &mut String| {
        let count = component.chars().count();
        if count > MAX_COMPONENT {
            out.extend(component.chars().take(32));
            out.push_str("...");
            out.extend(component.chars().skip(count - 8));
        } else {
            out.push_str(component);
        }
        component.clear();
    };
    for c in text.chars() {
        if c == '/' || c == '\\' {
            flush(&mut component, &mut out);
            out.push(c);
        } else {
            component.push(c);
        }
    }
    flush(&mut component, &mut out);
    out
}

/// Serialize errors as their string message so JS receives a plain string.
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn long_components_are_shortened_in_messages() {
        let blob = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8".repeat(4);
        let path = Path::new("D:\\lib\\models\\key").join(format!("data:image/png;base64,{blob}"));
        let text = short_path(&path);
        assert!(text.len() < 100, "{text}");
        assert!(text.starts_with("D:\\lib\\models\\key"), "{text}");
        assert!(text.contains("data:image/png;base64,iV") && text.ends_with("..."), "{text}");
        let long_name = Path::new("lib").join("x".repeat(200));
        let text = short_path(&long_name);
        assert!(text.len() < 60 && text.contains("..."), "{text}");
        assert_eq!(short_path(Path::new("library/models/ab/Crate.gltf")), "library/models/ab/Crate.gltf");
        let error = AppError::file("Resolve asset file", &path, std::io::Error::other("bad"));
        assert!(error.to_string().len() < 140, "{error}");
    }
}
