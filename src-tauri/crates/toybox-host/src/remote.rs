//! Remote libraries: check a server, reconnect, manage downloaded files and (for an
//! admin token) the server's access tokens. The network work is `toybox_remote`;
//! every function here may use the network and blocks.

use toybox_core::error::{AppError, AppResult};
use toybox_core::infra::library::RemoteRole;
pub use toybox_remote::types::{NewToken, TokenView};
pub use toybox_remote::Probe;

use crate::host::Host;
use crate::libraries::LibraryInfo;

/// Check an address and token without saving anything: the server's name and the
/// role the token grants, and a warning when the address is not encrypted.
pub fn probe_remote_library(address: &str, token: &str) -> AppResult<Probe> {
    toybox_remote::probe(address, token).map_err(AppError::from)
}

impl Host {
    /// "Try again": ask the server who we are and report the library's state.
    pub fn reconnect_remote_library(&self, id: &str) -> AppResult<LibraryInfo> {
        self.remote(id)?.connect();
        self.library_info(id)
    }

    /// Bytes of files downloaded from a remote library.
    pub fn remote_cache_size(&self, id: &str) -> AppResult<u64> {
        Ok(self.remote(id)?.cache_size())
    }

    /// Delete the downloaded files of a remote library; returns the bytes freed.
    pub fn clear_remote_cache(&self, id: &str) -> AppResult<u64> {
        Ok(self.remote(id)?.clear_cache())
    }

    /// Make the files of these items available on this computer (a no-op for a
    /// local library). `include_sources` also fetches the source files kept with an
    /// item, and the files of the asset it was made from (`origin.derivedFrom`: the
    /// substance whose archive a material was rendered from).
    pub fn ensure_asset_files(&self, library: &str, asset_ids: &[String], include_sources: bool) -> AppResult<()> {
        let handle = self.resolve(library)?;
        if !handle.is_remote() {
            return Ok(());
        }
        toybox_core::ops::catalog::open_library(&handle)?;
        let store = toybox_core::infra::store::Store::for_handle(&handle);
        if !store.catalog_exists() {
            return Err(AppError::msg("The library has not been loaded yet"));
        }
        // Only the rows of the asked ids (items or variants) are read.
        let assets = store.load_assets(asset_ids)?;
        handle.ensure_assets(&assets)?;
        if include_sources {
            let mut sources = Vec::new();
            for asset in &assets {
                if let Some(origin) = &asset.origin {
                    sources.extend(origin.files.iter().cloned());
                }
            }
            handle.ensure_files(&sources)?;
            let mut parents: Vec<String> = Vec::new();
            for asset in &assets {
                for item in std::iter::once(asset).chain(&asset.variants) {
                    if let Some(parent) = item.origin.as_ref().and_then(|origin| origin.derived_from.clone()) {
                        if !parents.contains(&parent) && !asset_ids.contains(&parent) {
                            parents.push(parent);
                        }
                    }
                }
            }
            if !parents.is_empty() {
                // A parent that is not in the library (the link dangles) has nothing to fetch.
                handle.ensure_assets(&store.load_assets(&parents)?)?;
            }
        }
        Ok(())
    }

    /// The server's access tokens (needs the admin role).
    pub fn list_remote_tokens(&self, library: &str) -> AppResult<Vec<TokenView>> {
        self.remote(library)?.client().tokens().map_err(AppError::from)
    }

    /// Make a token. The secret is in the answer only this once.
    pub fn create_remote_token(&self, library: &str, name: &str, role: RemoteRole) -> AppResult<NewToken> {
        self.remote(library)?.client().create_token(name.trim(), role).map_err(AppError::from)
    }

    /// Revoke a token at once. The server refuses to revoke the last admin token.
    pub fn revoke_remote_token(&self, library: &str, token_id: &str) -> AppResult<()> {
        self.remote(library)?.client().revoke_token(token_id).map_err(AppError::from)
    }
}
