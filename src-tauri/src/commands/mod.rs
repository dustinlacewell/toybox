//! Command layer: thin orchestration exposed to the frontend. Each command
//! reads via infra, transforms via domain, writes via infra.

pub mod catalog;
pub mod catalog_build;
pub mod export_copy;
pub mod export_glb;
pub mod export_plugin;
pub mod export_util;
pub mod import;
pub mod packs;
pub mod plugins;
pub mod recenter;
pub mod scan;
pub mod thumbs;
