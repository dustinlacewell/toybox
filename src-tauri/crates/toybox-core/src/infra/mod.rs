//! Infrastructure layer: the side-effecting code (filesystem, app-data paths).
//! Nothing here depends on a GUI framework; callers pass explicit paths.

pub mod animation_import;
pub mod appdata;
pub mod audio;
#[cfg(test)]
mod audio_tests;
pub mod converter;
pub mod fsio;
pub mod library;
pub mod migrations;
pub mod registry;
pub mod retention;
pub mod settings;
pub mod store;
pub mod storage;
pub(crate) mod trace;
