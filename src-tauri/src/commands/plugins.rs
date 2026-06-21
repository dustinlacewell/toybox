//! Plugin discovery + the jailed filesystem surface plugins call through
//! `ctx.fs`. Plugins are folders under `<app-data>/plugins/<id>/` each carrying a
//! `manifest.json` and an entry module.
//!
//! Filesystem boundary (the real one — JS capability flags are advisory):
//! - `plugin_read_text` is jailed to `plugins_dir` (the loader reads plugin
//!   source through it; a plugin can't read arbitrary files by spoofing a path).
//! - `plugin_write_*` accept an `authorized_root` (the user-picked target dir or
//!   save-file the current run is scoped to) and reject any path that escapes it.
//! - The `plugin://` protocol handler (in `lib.rs`) is jailed to `plugins_dir`
//!   the same way.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::infra::{appdata, fsio};

/// A discovered plugin's validated manifest plus the absolute path of its entry
/// module (so the loader can address it via the `plugin://` protocol).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifestDto {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub permissions: serde_json::Value,
    pub fields: serde_json::Value,
    /// Plugin-shipped slot UI modules ({ exportPanel?, importPanel? }), opaque
    /// here — the frontend loader validates and mounts them.
    pub ui: serde_json::Value,
    /// Absolute path to the entry module on disk (entry resolved under the dir).
    pub entry_abs_path: String,
}

/// Enumerate `<app-data>/plugins/*`, reading and validating each `manifest.json`.
/// A directory without a readable/valid manifest, a mismatched id, or a missing
/// entry file is skipped (not an error) so one bad plugin never breaks discovery.
#[tauri::command]
pub async fn list_plugins(app: AppHandle) -> AppResult<Vec<PluginManifestDto>> {
    let dir = appdata::plugins_dir(&app)?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        // `entry.path().is_dir()` (not `file_type().is_dir()`) so a directory
        // junction/symlink — how the dev tooling links in-repo plugins on
        // Windows — is followed and counts as a plugin dir. `file_type()` does
        // not traverse reparse points, so a junction would report as a symlink
        // and be skipped.
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(m) = read_manifest(&path) {
            out.push(m);
        }
    }
    Ok(out)
}

/// Read + validate one plugin dir's `manifest.json`. Returns `None` on any
/// problem (so discovery is tolerant); detailed surfacing is the loader's job.
fn read_manifest(plugin_dir: &Path) -> Option<PluginManifestDto> {
    let dir_name = plugin_dir.file_name()?.to_str()?.to_string();
    let manifest_path = plugin_dir.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;

    let id = doc.get("id")?.as_str()?.to_string();
    if id != dir_name {
        return None; // id must match the folder it lives in
    }
    let name = doc.get("name")?.as_str()?.to_string();
    let version = doc.get("version")?.as_str()?.to_string();
    let kind = doc.get("kind")?.as_str()?.to_string();
    if kind != "exporter" && kind != "importer" {
        return None;
    }
    // Exporters load their entry module (it carries `run`); importers are
    // panel-only (their `ui.importPanel` does the work), so `entry` is optional
    // and may be absent on disk.
    let entry = doc.get("entry").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let entry_abs = plugin_dir.join(&entry);
    if kind == "exporter" && (entry.is_empty() || !entry_abs.exists()) {
        return None;
    }

    Some(PluginManifestDto {
        id,
        name,
        version,
        kind,
        entry,
        description: doc.get("description").and_then(|v| v.as_str()).map(String::from),
        permissions: doc.get("permissions").cloned().unwrap_or_else(|| serde_json::json!({})),
        fields: doc.get("fields").cloned().unwrap_or_else(|| serde_json::json!([])),
        ui: doc.get("ui").cloned().unwrap_or_else(|| serde_json::json!({})),
        entry_abs_path: entry_abs.to_string_lossy().to_string(),
    })
}

// --- jailed filesystem surface --------------------------------------------

/// Read a UTF-8 file the plugin loader needs, jailed to `plugins_dir`.
#[tauri::command]
pub async fn plugin_read_text(app: AppHandle, path: String) -> AppResult<String> {
    let root = appdata::plugins_dir(&app)?;
    let abs = jail(&root, &path)?;
    fsio::read_text(&abs)
}

/// Existence check, jailed to `plugins_dir`.
#[tauri::command]
pub async fn plugin_exists(app: AppHandle, path: String) -> AppResult<bool> {
    let root = appdata::plugins_dir(&app)?;
    // exists() must not error for a missing file; only reject jail escapes.
    Ok(jail(&root, &path).map(|p| fsio::exists(&p)).unwrap_or(false))
}

/// Write bytes under the run's `authorized_root` (the user-picked target). Any
/// `path` that escapes `authorized_root` is rejected.
#[tauri::command]
pub async fn plugin_write_bytes(
    authorized_root: String,
    path: String,
    bytes: Vec<u8>,
) -> AppResult<()> {
    let abs = jail(Path::new(&authorized_root), &path)?;
    fsio::write_bytes(&abs, &bytes)
}

/// Write text under the run's `authorized_root`, same jail as `plugin_write_bytes`.
#[tauri::command]
pub async fn plugin_write_text(
    authorized_root: String,
    path: String,
    text: String,
) -> AppResult<()> {
    let abs = jail(Path::new(&authorized_root), &path)?;
    fsio::write_text(&abs, &text)
}

/// Resolve `path` under `root` and confirm it does not escape. `path` may be
/// absolute (must then be inside `root`) or relative (joined onto `root`). The
/// check is lexical — no component may be `..` and an absolute path must share
/// `root`'s prefix — so it holds even for not-yet-existing files (canonicalize
/// would fail on those).
fn jail(root: &Path, path: &str) -> AppResult<PathBuf> {
    let candidate = Path::new(path);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };

    if joined.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(AppError::msg(format!("path escapes its jail: {path}")));
    }
    let root_norm = normalize(root);
    let joined_norm = normalize(&joined);
    if !joined_norm.starts_with(&root_norm) {
        return Err(AppError::msg(format!(
            "path '{path}' is outside the authorized root"
        )));
    }
    Ok(joined)
}

/// Lexical normalization: collapse `.` segments and compare case-insensitively
/// on Windows by lowercasing. No filesystem access (works for missing files).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::Normal(s) => {
                #[cfg(windows)]
                out.push(s.to_string_lossy().to_lowercase());
                #[cfg(not(windows))]
                out.push(s);
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jail_allows_paths_under_root() {
        let root = Path::new(r"C:\proj\out");
        assert!(jail(root, "sub/file.glb").is_ok());
        assert!(jail(root, r"C:\proj\out\sub\file.glb").is_ok());
    }

    #[test]
    fn jail_rejects_parent_escape() {
        let root = Path::new(r"C:\proj\out");
        assert!(jail(root, "../evil.txt").is_err());
        assert!(jail(root, "sub/../../evil.txt").is_err());
    }

    #[test]
    fn jail_rejects_absolute_outside_root() {
        let root = Path::new(r"C:\proj\out");
        assert!(jail(root, r"C:\windows\system32\evil.dll").is_err());
    }
}
