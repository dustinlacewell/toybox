//! Pure app-data path helpers. Where the app-data directory itself lives is the
//! host's business (the desktop app asks Tauri, a server takes a flag); this
//! module only knows the layout under it. Per-library data (catalog cache,
//! thumbnails) lives under `<app-data>/libraries/<id>/`; see `infra::registry`.

use std::path::{Path, PathBuf};

/// The settings JSON path: `<app-data>/settings.json` (holds the library registry).
pub fn settings_path(app_data: &Path) -> PathBuf {
    app_data.join("settings.json")
}

/// Make an asset id safe as a single path segment.
pub fn sanitize_id(asset_id: &str) -> String {
    asset_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}
