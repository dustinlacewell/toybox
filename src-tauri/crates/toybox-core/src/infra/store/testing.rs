//! Helpers for tests that used to write the manifest and the cache as JSON files:
//! the same documents, put through the store.

use std::path::Path;

use serde_json::{json, Value};

use super::{read_manifest, write_manifest, Store};
use crate::domain::catalog_model::Catalog;

/// Make the manifest of the library at `root` equal this document.
pub(crate) fn put_manifest(root: &Path, doc: &Value) {
    write_manifest(root, doc).unwrap();
}

pub(crate) fn get_manifest(root: &Path) -> Value {
    read_manifest(root).unwrap()
}

/// A catalog document as the old cache file held it, completed where a test
/// left out what a stored catalog always has (the schema version, the rigs).
pub(crate) fn complete_catalog_doc(mut doc: Value) -> Value {
    let object = doc.as_object_mut().expect("a catalog document is an object");
    object.entry("schemaVersion").or_insert_with(|| json!(crate::config::SCHEMA_VERSION));
    object.entry("libraryRoot").or_insert_with(|| json!(""));
    object.entry("assets").or_insert_with(|| json!([]));
    doc
}

/// Save a catalog document as the catalog of the library whose cache file is `cache`.
pub(crate) fn put_catalog_doc(root: &Path, cache: &Path, doc: Value) {
    let catalog: Catalog = serde_json::from_value(crate::domain::catalog_migrate::upgrade(complete_catalog_doc(doc))).unwrap();
    Store::local(root, cache).save_catalog(&catalog).unwrap();
}

/// The saved catalog of the library whose cache file is `cache`, as a document.
pub(crate) fn get_catalog_doc(root: &Path, cache: &Path) -> Value {
    serde_json::to_value(Store::local(root, cache).load_catalog().unwrap().expect("a catalog was saved")).unwrap()
}
