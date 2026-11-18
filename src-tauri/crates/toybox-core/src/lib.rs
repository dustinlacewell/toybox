//! Toybox library logic with no GUI dependency: the catalog model, library
//! storage and migrations, and the operations that act on one library given its
//! paths. The desktop app (`toybox`, src-tauri) wraps these in Tauri commands;
//! a headless server uses them directly.

pub mod config;
pub mod domain;
pub mod error;
pub mod infra;
pub mod maintenance;
pub mod maintenance_colors;
pub mod ops;

#[cfg(test)]
mod animation_library_tests;
