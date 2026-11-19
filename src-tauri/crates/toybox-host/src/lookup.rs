//! Finding a registered library by what a person types: its id or its name.

use std::fmt;

use toybox_core::infra::library::LibraryInfo;

use crate::host::Host;

/// Why a library could not be picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupError {
    /// No library is registered at all.
    NoLibraries,
    /// No default library is set and there is more than one library.
    NoDefault,
    /// Nothing has this id or name.
    NotFound(String),
    /// Several libraries have this name (their ids are given).
    Ambiguous(String, Vec<String>),
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LookupError::NoLibraries => f.write_str("no library is registered"),
            LookupError::NoDefault => f.write_str("there is more than one library and none is the default"),
            LookupError::NotFound(text) => write!(f, "no library is named or has the id '{text}'"),
            LookupError::Ambiguous(text, ids) => write!(f, "'{text}' matches more than one library ({})", ids.join(", ")),
        }
    }
}

impl std::error::Error for LookupError {}

impl Host {
    /// The library with this id, or else the one whose name matches `text`
    /// case-insensitively. A name must be unambiguous.
    pub fn find_library(&self, text: &str) -> Result<LibraryInfo, LookupError> {
        let all = self.list_libraries();
        if all.is_empty() {
            return Err(LookupError::NoLibraries);
        }
        let text = text.trim();
        if let Some(found) = all.iter().find(|library| library.id == text) {
            return Ok(found.clone());
        }
        let lower = text.to_lowercase();
        let named: Vec<&LibraryInfo> = all.iter().filter(|library| library.name.to_lowercase() == lower).collect();
        match named.as_slice() {
            [] => Err(LookupError::NotFound(text.to_string())),
            [one] => Ok((*one).clone()),
            many => Err(LookupError::Ambiguous(text.to_string(), many.iter().map(|library| library.id.clone()).collect())),
        }
    }

    /// The library `text` names, or (with `None`) the default library; when there is
    /// no default but exactly one library, that one.
    pub fn target_library(&self, text: Option<&str>) -> Result<LibraryInfo, LookupError> {
        if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
            return self.find_library(text);
        }
        let all = self.list_libraries();
        match all.as_slice() {
            [] => Err(LookupError::NoLibraries),
            [only] => Ok(only.clone()),
            many => many.iter().find(|library| library.is_default).cloned().ok_or(LookupError::NoDefault),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::HostPaths;
    use std::sync::Arc;
    use toybox_remote::secrets::MemoryStore;

    #[test]
    fn libraries_are_found_by_id_or_by_name() {
        let base = std::env::temp_dir().join(format!("toybox-lookup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let host = Host::open_lazy(HostPaths::for_home(base.join("home")), Arc::new(MemoryStore::new()), None).unwrap();
        assert_eq!(host.find_library("x").unwrap_err(), LookupError::NoLibraries);
        assert_eq!(host.target_library(None).unwrap_err(), LookupError::NoLibraries);
        let one = host.create_library(&base.join("one"), Some("Main")).unwrap();
        let two = host.create_library(&base.join("two"), Some("Other")).unwrap();
        assert_eq!(host.find_library(&one.id).unwrap().id, one.id);
        assert_eq!(host.find_library("MAIN").unwrap().id, one.id);
        assert_eq!(host.find_library("nope").unwrap_err(), LookupError::NotFound("nope".into()));
        // The first library is the default.
        assert_eq!(host.target_library(None).unwrap().id, one.id);
        assert_eq!(host.target_library(Some(&two.id)).unwrap().id, two.id);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_name_two_libraries_share_is_ambiguous() {
        let base = std::env::temp_dir().join(format!("toybox-lookup-ambiguous-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let host = Host::open_lazy(HostPaths::for_home(base.join("home")), Arc::new(MemoryStore::new()), None).unwrap();
        host.create_library(&base.join("a"), Some("Same")).unwrap();
        host.create_library(&base.join("b"), Some("same")).unwrap();
        assert!(matches!(host.find_library("SAME"), Err(LookupError::Ambiguous(..))));
        let _ = std::fs::remove_dir_all(base);
    }
}
