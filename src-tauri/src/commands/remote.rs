//! Remote-library commands: connect a server, check or change its token, retry
//! a connection, manage downloaded files and (for an admin token) the server's
//! access tokens. The network work is `toybox_remote`; every call that uses the
//! network runs on a blocking thread so the interface never waits on it.

use tauri::AppHandle;
use toybox_core::infra::library::RemoteRole;
use toybox_remote::types::{NewToken, TokenView};
use toybox_remote::Probe;

use crate::error::{AppError, AppResult};
use crate::infra::library::{self, LibraryInfo};

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
    blocking(move || toybox_remote::probe(&address, &token).map_err(AppError::from)).await
}

/// Add a remote library. The token goes to the OS credential store, never to
/// settings; the address (normalised) to the registry.
#[tauri::command]
pub async fn add_remote_library(app: AppHandle, address: String, token: String, name: Option<String>) -> AppResult<LibraryInfo> {
    blocking(move || {
        let prepared = toybox_remote::hub::prepare(&address, &token, name.as_deref()).map_err(AppError::from)?;
        library::add_remote(&app, &prepared, token.trim())
    })
    .await
}

/// Use another access token for a remote library (checked with the server first).
#[tauri::command]
pub async fn change_remote_token(app: AppHandle, library: String, token: String) -> AppResult<LibraryInfo> {
    blocking(move || library::change_token(&app, &library, &token)).await
}

/// "Try again": ask the server who we are and report the library's state.
#[tauri::command]
pub async fn reconnect_remote_library(app: AppHandle, library: String) -> AppResult<LibraryInfo> {
    blocking(move || {
        library::remote(&app, &library)?.connect();
        library::info_of(&app, &library)
    })
    .await
}

/// Bytes of files downloaded from a remote library.
#[tauri::command]
pub async fn remote_cache_size(app: AppHandle, library: String) -> AppResult<u64> {
    blocking(move || Ok(library::remote(&app, &library)?.cache_size())).await
}

/// Delete the downloaded files of a remote library; returns the bytes freed.
#[tauri::command]
pub async fn clear_remote_cache(app: AppHandle, library: String) -> AppResult<u64> {
    blocking(move || Ok(library::remote(&app, &library)?.clear_cache())).await
}

/// Make the files of these items available on this computer (a no-op for a
/// local library). `include_sources` also fetches the source files kept with an
/// item (for example the archive a material was rendered from).
#[tauri::command]
pub async fn ensure_asset_files(app: AppHandle, library: String, asset_ids: Vec<String>, include_sources: bool) -> AppResult<()> {
    blocking(move || {
        let handle = library::resolve(&app, &library)?;
        if !handle.is_remote() {
            return Ok(());
        }
        toybox_core::ops::catalog::open_library(&handle)?;
        let store = toybox_core::infra::store::Store::for_handle(&handle);
        if !store.catalog_exists() {
            return Err(AppError::msg("The library has not been loaded yet"));
        }
        // Only the rows of the asked ids (items or variants) are read.
        let assets = store.load_assets(&asset_ids)?;
        handle.ensure_assets(&assets)?;
        if include_sources {
            let mut sources = Vec::new();
            for asset in &assets {
                if let Some(origin) = &asset.origin {
                    sources.extend(origin.files.iter().cloned());
                }
            }
            handle.ensure_files(&sources)?;
        }
        Ok(())
    })
    .await
}

/// The server's access tokens (needs the admin role).
#[tauri::command]
pub async fn list_remote_tokens(app: AppHandle, library: String) -> AppResult<Vec<TokenView>> {
    blocking(move || library::remote(&app, &library)?.client().tokens().map_err(AppError::from)).await
}

/// Make a token. The secret is in the answer only this once.
#[tauri::command]
pub async fn create_remote_token(app: AppHandle, library: String, name: String, role: RemoteRole) -> AppResult<NewToken> {
    blocking(move || library::remote(&app, &library)?.client().create_token(name.trim(), role).map_err(AppError::from)).await
}

/// Revoke a token at once. The server refuses to revoke the last admin token.
#[tauri::command]
pub async fn revoke_remote_token(app: AppHandle, library: String, token_id: String) -> AppResult<()> {
    blocking(move || library::remote(&app, &library)?.client().revoke_token(&token_id).map_err(AppError::from)).await
}
