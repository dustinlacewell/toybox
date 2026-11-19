//! Services a plugin can ask the host for beyond the jailed filesystem, as Tauri
//! commands: the settings a plugin declares, running a program the user pointed a
//! setting at, local-network requests and a scratch folder. The work and every
//! permission check are `toybox_host::plugin_services`.

use serde_json::{Map, Value};
use tauri::AppHandle;
use toybox_host::plugin_services::{self, ToolResult};

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::error::AppResult;

/// A plugin's settings: what the user chose, else each declared default.
#[tauri::command]
pub fn get_plugin_settings(app: AppHandle, plugin_id: String) -> AppResult<Map<String, Value>> {
    host(&app).plugin_settings(&plugin_id)
}

/// Store one setting of a plugin; `null` returns it to its default. Only keys
/// the plugin's manifest declares are accepted.
#[tauri::command]
pub fn set_plugin_setting(app: AppHandle, plugin_id: String, key: String, value: Value) -> AppResult<()> {
    host(&app).set_plugin_setting(&plugin_id, &key, value)
}

/// Run the program a `file` setting of the plugin points at, with `args`, in
/// that program's own folder, and wait for it. A program still running after
/// `timeout_ms` (two minutes when 0, at most 30 minutes) is killed and the call
/// fails. A non-zero exit code is a normal result.
#[tauri::command]
pub async fn plugin_run_tool(app: AppHandle, plugin_id: String, setting: String, args: Vec<String>, timeout_ms: u64) -> AppResult<ToolResult> {
    let host = host(&app);
    blocking(move || host.run_plugin_tool(&plugin_id, &setting, &args, timeout_ms)).await
}

/// Start the program a `file` setting of the plugin points at, with `args`, in
/// that program's own folder. It runs in the background until it exits or the
/// app closes. The path always comes from the user's setting (or the default
/// the manifest declares), never from the call.
#[tauri::command]
pub fn plugin_launch_tool(app: AppHandle, plugin_id: String, setting: String, args: Vec<String>) -> AppResult<()> {
    host(&app).launch_plugin_tool(&plugin_id, &setting, &args)
}

/// Toybox's own process id. A program a plugin starts may want to know which
/// process it is working for, to stop when that process goes away.
#[tauri::command]
pub fn plugin_host_process_id() -> u32 {
    plugin_services::host_process_id()
}

/// Whether something on this machine is listening on `port`.
#[tauri::command]
pub async fn plugin_local_port_open(app: AppHandle, plugin_id: String, port: u16) -> AppResult<bool> {
    let host = host(&app);
    blocking(move || host.plugin_local_port_open(&plugin_id, port)).await
}

/// Send one JSON message to a service on this machine and return its JSON
/// reply. One connection per message; the reply ends when it parses or the
/// service closes the connection.
#[tauri::command]
pub async fn plugin_local_json(app: AppHandle, plugin_id: String, port: u16, message: Value, timeout_ms: u64) -> AppResult<Value> {
    let host = host(&app);
    blocking(move || host.plugin_local_json(&plugin_id, port, &message, timeout_ms)).await
}

/// The plugin's private folder for intermediate files, created on demand.
/// `clear` empties it first. Nothing here is kept: the host may empty it too.
#[tauri::command]
pub fn plugin_scratch_dir(app: AppHandle, plugin_id: String, clear: bool) -> AppResult<String> {
    host(&app).plugin_scratch_dir(&plugin_id, clear)
}
