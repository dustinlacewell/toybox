//! Static configuration. The library root is pinned here and must stay in sync
//! with the asset-protocol scope in `tauri.conf.json` (the webview can only load
//! files under that scope).

/// Absolute path to the asset library root.
pub const LIBRARY_ROOT: &str = r"D:\code\games\assets";

/// Capital's native catalog source file, relative to `LIBRARY_ROOT`.
pub const SEED_REL: &str = "_library_config/catalog.json";

/// Catalog schema version we write.
pub const SCHEMA_VERSION: u32 = 1;
