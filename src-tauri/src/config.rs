//! Static configuration. The library root is pinned here and must stay in sync
//! with the asset-protocol scope in `tauri.conf.json` (the webview can only load
//! files under that scope).

/// Absolute path to the asset library root.
pub const LIBRARY_ROOT: &str = r"D:\code\games\assets";

/// The Godot curation file, relative to `LIBRARY_ROOT`.
pub const SEED_REL: &str = "_library_config/asset_library.json";

/// Catalog schema version we write.
pub const SCHEMA_VERSION: u32 = 1;
