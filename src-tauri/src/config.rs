//! Static configuration. The library *root* is no longer pinned here — it is a
//! runtime, user-chosen value (see `infra::library`). What remains is genuinely
//! static: the root-relative location of the seed file and the cache schema
//! version.

/// Toybox's native catalog source file, relative to the library root.
pub const SEED_REL: &str = "_library_config/catalog.json";

/// Catalog schema version we write. Bump whenever the *derivation* of any cached
/// field changes (not just its shape), so older caches built under the old rules
/// are rejected on load and rescanned. v2 introduced the animation channel filter
/// (empty FBX "Take 001" no longer counts as a clip); a v1 cache could carry a
/// stale `clipCount > 0` for assets with no real animation.
pub const SCHEMA_VERSION: u32 = 2;
