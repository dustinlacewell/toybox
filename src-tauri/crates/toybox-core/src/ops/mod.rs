//! Library operations: what the desktop commands and a server both do with a
//! `LibraryHandle` (or explicit paths). Each module reads via `infra`,
//! transforms via `domain`, writes via `infra`; none knows about a GUI host.

pub mod animation_preview;
pub mod browse;
pub mod catalog;
pub mod catalog_build;
pub mod content_hash;
pub mod delete;
mod delete_files;
pub mod export_copy;
pub mod export_glb;
pub mod export_plugin;
pub mod export_util;
pub mod files;
pub mod import;
pub mod import_convert;
pub mod metadata;
pub mod packs;
pub mod preview_source;
pub mod recenter;
pub mod scan;
pub mod thumbs;
pub mod transfer;
#[cfg(test)]
mod content_hash_tests;
#[cfg(test)]
mod incremental_tests;
#[cfg(test)]
mod library_tests;
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod substance_tests;
