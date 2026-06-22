mod commands;
mod config;
mod domain;
mod error;
mod infra;

use commands::{
    catalog, export_copy, export_glb, export_plugin, import, library, packs, plugins, recenter,
    scan, thumbs,
};
use infra::library::LibraryState;

/// Re-exports of the pure domain modules for integration tests in `tests/`.
/// Integration tests link against the public crate API only.
pub mod testing {
    pub use crate::domain::{export_copy_plan, glb_assemble, gltf_parse, image_embed, paths};
}

/// Origin-correction domain surface for integration tests.
pub mod testing_origin {
    pub use crate::domain::gltf_origin::{recenter, world_aabb, Aabb, Align, Axis};
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(LibraryState::default())
        .setup(|app| {
            // Restore the persisted library root (if any) and re-open the asset
            // scope to it, so a returning user skips the picker and the viewer
            // can serve files immediately.
            infra::library::hydrate(app.handle())?;
            Ok(())
        })
        .register_uri_scheme_protocol("plugin", serve_plugin_asset)
        .invoke_handler(tauri::generate_handler![
            library::get_library_root,
            library::set_library_root,
            scan::scan_library,
            catalog::load_catalog,
            catalog::save_catalog,
            catalog::library_root,
            catalog::resolve_asset_path,
            catalog::thumb_dir,
            catalog::thumb_path,
            thumbs::save_thumb,
            thumbs::set_thumb_state,
            thumbs::list_pending_thumbs,
            thumbs::clear_thumbs,
            export_copy::export_copy,
            export_glb::export_glb,
            recenter::recenter_asset,
            packs::load_packs,
            import::merge_seed_entries,
            // Plugin system: discovery + jailed fs.
            plugins::list_plugins,
            plugins::plugin_read_text,
            plugins::plugin_exists,
            plugins::plugin_write_bytes,
            plugins::plugin_write_text,
            // Per-asset export primitives plugins orchestrate.
            export_plugin::read_asset_gltf,
            export_plugin::assemble_glb_for_asset,
            export_plugin::perform_asset_copy,
            export_plugin::transcode_image,
            export_plugin::placer_merge_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Serve plugin module source over `plugin://localhost/<id>/<rel>` so the webview
/// loads it from a real origin (subject to the document import map, unlike a blob
/// URL — which is why bare `import "three"` resolves inside plugin code). Jailed
/// to `<app-data>/plugins`: any `..` escape or out-of-root path 404s.
fn serve_plugin_asset(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let app = ctx.app_handle();
    match resolve_plugin_file(app, request.uri().path()) {
        Some((bytes, mime)) => tauri::http::Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("Access-Control-Allow-Origin", "*")
            .body(bytes)
            .unwrap_or_else(|_| not_found()),
        None => not_found(),
    }
}

/// Map a `plugin://localhost/<id>/<rel>` path to bytes + MIME under the jailed
/// plugins dir. Returns `None` for any escape, missing file, or read error.
fn resolve_plugin_file(
    app: &tauri::AppHandle,
    uri_path: &str,
) -> Option<(Vec<u8>, &'static str)> {
    let root = infra::appdata::plugins_dir(app).ok()?;
    // uri_path is like "/com.toybox.placer/index.js"; reject parent escapes.
    let rel = uri_path.trim_start_matches('/');
    if rel.split('/').any(|s| s == ".." || s.is_empty()) {
        return None;
    }
    let abs = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !abs.starts_with(&root) {
        return None;
    }
    let bytes = std::fs::read(&abs).ok()?;
    Some((bytes, mime_for(&abs)))
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("js") | Some("mjs") => "text/javascript",
        Some("json") => "application/json",
        Some("css") => "text/css",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn not_found() -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(404)
        .body(Vec::new())
        .expect("static 404 response")
}
