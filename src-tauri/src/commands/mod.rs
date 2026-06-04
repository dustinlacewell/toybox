//! Command layer: thin orchestration exposed to the frontend. Each command
//! reads via infra, transforms via domain, writes via infra.

pub mod catalog;
pub mod export_copy;
pub mod export_glb;
pub mod export_placer;
pub mod export_util;
pub mod packs;
pub mod recenter;
pub mod scan;
pub mod thumbs;
