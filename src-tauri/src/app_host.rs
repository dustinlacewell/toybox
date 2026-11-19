//! The app's side of the `Host`: build it from the folders Tauri reports, keep it in
//! Tauri state, and do the steps only a window can (the asset-protocol scope).

use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use toybox_host::{Host, HostPaths};

use crate::error::{AppError, AppResult};

/// Event name progress of remote libraries is emitted under.
pub const PROGRESS_EVENT: &str = "library-progress";

/// The managed `Host`.
pub fn host(app: &AppHandle) -> Arc<Host> {
    app.state::<Arc<Host>>().inner().clone()
}

/// Called once from `.setup()`: restore the registered libraries (migrating an old
/// single-library setup), re-open the asset scope to each root so a returning user
/// skips the picker and the viewer can serve files immediately, keep the `Host` in
/// state and install the bundled plugins into the user's plugins dir on first run.
/// An error here aborts window creation, so a step that can fail is best-effort
/// inside the host.
pub fn setup(app: &AppHandle) -> AppResult<()> {
    let path = app.path();
    let paths = HostPaths::new(
        path.app_data_dir().map_err(|e| AppError::msg(format!("app_data_dir: {e}")))?,
        path.app_cache_dir().map_err(|e| AppError::msg(format!("app_cache_dir: {e}")))?,
    );
    let events = app.clone();
    let sink: toybox_remote::progress::Sink = Arc::new(move |progress| {
        let _ = events.emit(PROGRESS_EVENT, progress);
    });
    let host = Host::open(paths, Some(sink))?;
    for root in host.local_library_roots() {
        extend_scope(app, &root);
    }
    // Remote libraries keep downloaded files in the app cache; the viewer loads them from there.
    extend_scope(app, &host.paths().remote_files_dir());
    app.manage(Arc::new(host));
    if let Ok(resources) = path.resource_dir() {
        host_seed(app, &resources)?;
    }
    Ok(())
}

fn host_seed(app: &AppHandle, resources: &Path) -> AppResult<()> {
    host(app).seed_bundled_plugins(resources)
}

/// Allow the webview's asset protocol to serve files under `root`. Without this
/// the catalog scans fine but every gltf/texture/thumbnail 404s in the viewer.
pub fn extend_scope(app: &AppHandle, root: &Path) {
    // Best-effort: a scope error here shouldn't abort adopting the library; the
    // viewer would degrade, but scanning/exports still work.
    let _ = app.asset_protocol_scope().allow_directory(root, true);
}
