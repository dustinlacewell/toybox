//! Static configuration. The library root is pinned here and must stay in sync
//! with the asset-protocol scope in `tauri.conf.json` (the webview can only load
//! files under that scope).

/// Absolute path to the asset library root.
pub const LIBRARY_ROOT: &str = r"D:\code\games\assets";

/// Toybox's native catalog source file, relative to `LIBRARY_ROOT`.
pub const SEED_REL: &str = "_library_config/catalog.json";

/// Catalog schema version we write. Bump whenever the *derivation* of any cached
/// field changes (not just its shape), so older caches built under the old rules
/// are rejected on load and rescanned. v2 introduced the animation channel filter
/// (empty FBX "Take 001" no longer counts as a clip); a v1 cache could carry a
/// stale `clipCount > 0` for assets with no real animation.
pub const SCHEMA_VERSION: u32 = 2;
