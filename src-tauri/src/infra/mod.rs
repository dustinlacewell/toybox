//! Infrastructure layer. The path-based, framework-free modules live in
//! `toybox-core` and are re-exported here under their old names; what remains
//! in this crate is what needs a Tauri `AppHandle` (app-data location, live
//! library state, bundled plugins).

pub use toybox_core::infra::{fsio, registry};

pub mod appdata;
pub mod bundled;
pub mod library;
pub mod settings;
