//! Plugin discovery + the jailed filesystem surface plugins call through
//! `ctx.fs`, as Tauri commands. The work, the permission checks, the jail and the
//! dependency rules are `toybox_host::{plugins, plugin_fs}`, so the command line
//! behaves identically.

use std::path::Path;

use tauri::AppHandle;
use toybox_host::plugin_fs::{self, DirEntryDto};
use toybox_host::plugins::PluginManifestDto;

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::error::AppResult;

/// Enumerate `<app-data>/plugins/*`, reading and validating each `manifest.json`
/// and resolving dependencies.
#[tauri::command]
pub async fn list_plugins(app: AppHandle) -> AppResult<Vec<PluginManifestDto>> {
    host(&app).list_plugins()
}

/// List one directory level under a user-authorized `source_root` (the folder
/// the user picked to import from). Jailed to `source_root` the same lexical way
/// writes are. Importers use this to walk a source tree they don't own.
#[tauri::command]
pub async fn plugin_read_dir(source_root: String, path: String) -> AppResult<Vec<DirEntryDto>> {
    plugin_fs::read_dir_under(Path::new(&source_root), &path)
}

/// Read a file's bytes under a user-authorized `source_root`. Same jail as
/// `plugin_read_dir`; importers use it to pull source asset bytes for copy/unpack.
///
/// Returned as a raw binary response: a JSON number array would be several
/// times the file's size, and importers read whole HDR images through this.
#[tauri::command]
pub async fn plugin_read_bytes(source_root: String, path: String) -> AppResult<tauri::ipc::Response> {
    Ok(tauri::ipc::Response::new(plugin_fs::read_bytes_under(Path::new(&source_root), &path)?))
}

/// Copy one file from a user-authorized `source_root` to a path under
/// `authorized_root`, without its bytes crossing the IPC boundary. Both ends
/// are jailed exactly like `plugin_read_bytes` and `plugin_write_bytes`. An
/// existing destination is only accepted when it already holds the same bytes.
#[tauri::command]
pub async fn plugin_copy_file(
    source_root: String,
    path: String,
    authorized_root: String,
    target: String,
) -> AppResult<()> {
    blocking(move || plugin_fs::copy_file_under(Path::new(&source_root), &path, Path::new(&authorized_root), &target)).await
}

/// Read an image under `source_root` as PNG bytes scaled down to fit
/// `max_size` x `max_size`. Same jail as `plugin_read_bytes`; previews use it
/// instead of shipping a whole texture through IPC.
#[tauri::command]
pub async fn plugin_image_thumb(
    source_root: String,
    path: String,
    max_size: u32,
) -> AppResult<Vec<u8>> {
    plugin_fs::image_thumb_under(Path::new(&source_root), &path, max_size)
}

/// Read a UTF-8 file the plugin loader needs, jailed to `plugins_dir`.
#[tauri::command]
pub async fn plugin_read_text(app: AppHandle, path: String) -> AppResult<String> {
    host(&app).plugin_read_text(&path)
}

/// Existence check, jailed to `plugins_dir`.
#[tauri::command]
pub async fn plugin_exists(app: AppHandle, path: String) -> AppResult<bool> {
    Ok(host(&app).plugin_exists(&path))
}

/// Write bytes under the run's `authorized_root` (the user-picked target). Any
/// `path` that escapes `authorized_root` is rejected.
#[tauri::command]
pub async fn plugin_write_bytes(
    authorized_root: String,
    path: String,
    bytes: Vec<u8>,
) -> AppResult<()> {
    plugin_fs::write_bytes_under(Path::new(&authorized_root), &path, &bytes)
}

/// Write text under the run's `authorized_root`, same jail as `plugin_write_bytes`.
#[tauri::command]
pub async fn plugin_write_text(
    authorized_root: String,
    path: String,
    text: String,
) -> AppResult<()> {
    plugin_fs::write_text_under(Path::new(&authorized_root), &path, &text)
}
