//! Remote-library commands: connect a server, check or change its token, retry
//! a connection, manage downloaded files and (for an admin token) the server's
//! access tokens. The work is `toybox_host::Host`; every call that uses the
//! network runs on a blocking thread so the interface never waits on it.

use tauri::AppHandle;
use toybox_core::infra::library::RemoteRole;
use toybox_host::libraries::LibraryInfo;
use toybox_host::remote::{probe_remote_library as probe, NewToken, Probe, TokenView};

use crate::app_host::host;
use crate::error::{AppError, AppResult};

/// Run blocking work off the main thread and the async workers' fast path.
pub async fn blocking<T: Send + 'static>(work: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| AppError::msg(format!("Background task failed: {error}")))?
}

/// Check an address and token without saving anything: the server's name and the
/// role the token grants, and a warning when the address is not encrypted.
#[tauri::command]
pub async fn probe_remote_library(address: String, token: String) -> AppResult<Probe> {
    blocking(move || probe(&address, &token)).await
}

/// Add a remote library. The token goes to the OS credential store, never to
/// settings; the address (normalised) to the registry.
#[tauri::command]
pub async fn add_remote_library(app: AppHandle, address: String, token: String, name: Option<String>) -> AppResult<LibraryInfo> {
    let host = host(&app);
    blocking(move || host.add_remote_library(&address, &token, name.as_deref())).await
}

/// Use another access token for a remote library (checked with the server first).
#[tauri::command]
pub async fn change_remote_token(app: AppHandle, library: String, token: String) -> AppResult<LibraryInfo> {
    let host = host(&app);
    blocking(move || host.change_remote_token(&library, &token)).await
}

/// "Try again": ask the server who we are and report the library's state.
#[tauri::command]
pub async fn reconnect_remote_library(app: AppHandle, library: String) -> AppResult<LibraryInfo> {
    let host = host(&app);
    blocking(move || host.reconnect_remote_library(&library)).await
}

/// Bytes of files downloaded from a remote library.
#[tauri::command]
pub async fn remote_cache_size(app: AppHandle, library: String) -> AppResult<u64> {
    let host = host(&app);
    blocking(move || host.remote_cache_size(&library)).await
}

/// Delete the downloaded files of a remote library; returns the bytes freed.
#[tauri::command]
pub async fn clear_remote_cache(app: AppHandle, library: String) -> AppResult<u64> {
    let host = host(&app);
    blocking(move || host.clear_remote_cache(&library)).await
}

/// Make the files of these items available on this computer (a no-op for a
/// local library). `include_sources` also fetches the source files kept with an
/// item (for example the archive a material was rendered from).
#[tauri::command]
pub async fn ensure_asset_files(app: AppHandle, library: String, asset_ids: Vec<String>, include_sources: bool) -> AppResult<()> {
    let host = host(&app);
    blocking(move || host.ensure_asset_files(&library, &asset_ids, include_sources)).await
}

/// The server's access tokens (needs the admin role).
#[tauri::command]
pub async fn list_remote_tokens(app: AppHandle, library: String) -> AppResult<Vec<TokenView>> {
    let host = host(&app);
    blocking(move || host.list_remote_tokens(&library)).await
}

/// Make a token. The secret is in the answer only this once.
#[tauri::command]
pub async fn create_remote_token(app: AppHandle, library: String, name: String, role: RemoteRole) -> AppResult<NewToken> {
    let host = host(&app);
    blocking(move || host.create_remote_token(&library, &name, role)).await
}

/// Revoke a token at once. The server refuses to revoke the last admin token.
#[tauri::command]
pub async fn revoke_remote_token(app: AppHandle, library: String, token_id: String) -> AppResult<()> {
    let host = host(&app);
    blocking(move || host.revoke_remote_token(&library, &token_id)).await
}
