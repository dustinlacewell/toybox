//! Infrastructure layer: the only side-effecting code (filesystem, app-data).

pub mod appdata;
pub mod animation_import;
pub mod audio;
#[cfg(test)]
mod audio_tests;
pub mod bundled;
pub mod converter;
pub mod fsio;
pub mod library;
pub mod migrations;
pub mod storage;
pub mod settings;
