//! Command layer: thin Tauri wrappers exposed to the frontend. Each command
//! resolves its library from the app handle and calls the matching function in
//! `toybox_core::ops`, which holds the actual work.

pub mod browse;
pub mod catalog;
pub mod cli;
pub mod animation_preview;
pub mod export_copy;
pub mod export_glb;
pub mod export_plugin;
pub mod fingerprint;
pub mod import;
pub mod import_convert;
pub mod library;
pub mod metadata;
pub mod packs;
pub mod plugins;
pub mod plugin_services;
pub mod preview_source;
pub mod delete;
pub mod recenter;
pub mod remote;
pub mod scan;
pub mod settings;
pub mod thumbs;
pub mod transfer;
