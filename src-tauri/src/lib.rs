mod commands;
mod config;
mod domain;
mod error;
mod infra;
pub mod maintenance;
pub mod maintenance_colors;
#[cfg(test)]
mod animation_library_tests;

use commands::{
    catalog, export_copy, export_glb, export_plugin, import, import_convert, library, packs,
    plugins, recenter, scan, settings, thumbs,
};
use infra::library::LibraryState;

/// Re-exports of the pure domain modules for integration tests in `tests/`.
/// Integration tests link against the public crate API only.
pub mod testing {
    pub use crate::domain::{export_copy_plan, glb_assemble, gltf_parse, image_embed, paths};
}

pub mod testing_animation {
    pub use crate::domain::{catalog_model::{Catalog, AssetContent}, rig, glb_assemble};
    pub use crate::commands::animation_preview::{choices, read_preview};
    pub use crate::commands::export_util::{read_gltf, read_bin};
}

/// Origin-correction domain surface for integration tests.
pub mod testing_origin {
    pub use crate::domain::gltf_origin::{recenter, world_aabb, Aabb, Align, Axis};
}

/// FBX-conversion surface for the real end-to-end integration test.
pub mod testing_convert {
    pub use crate::commands::import_convert::promote;
    pub use crate::infra::converter::convert_fbx;
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(LibraryState::default())
        .setup(|app| {
            // Restore the persisted library root (if any) and re-open the asset
            // scope to it, so a returning user skips the picker and the viewer
            // can serve files immediately.
            infra::library::hydrate(app.handle())?;
            // Install bundled plugins (the importer) into the user's plugins dir
            // on first run, so a built app ships with a working import path.
            infra::bundled::seed_bundled_plugins(app.handle())?;
            Ok(())
        })
        .register_uri_scheme_protocol("plugin", serve_plugin_asset)
        .invoke_handler(tauri::generate_handler![
            library::get_library_root,
            library::set_library_root,
            library::create_library,
            settings::get_fbx2gltf_path,
            settings::set_fbx2gltf_path,
            settings::get_saved_exports,
            settings::set_saved_exports,
            import_convert::convert_to_gltf,
            scan::scan_library,
            catalog::load_catalog,
            commands::animation_preview::compatible_previews,
            commands::animation_preview::read_animation_preview,
            catalog::save_catalog,
            catalog::library_root,
            catalog::resolve_asset_path,
            catalog::thumb_dir,
            catalog::thumb_path,
            thumbs::save_thumb,
            thumbs::invalidate_thumb,
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
            plugins::plugin_read_dir,
            plugins::plugin_read_bytes,
            plugins::plugin_image_thumb,
            plugins::plugin_write_bytes,
            plugins::plugin_write_text,
            // Per-asset export primitives plugins orchestrate.
            export_plugin::read_asset_gltf,
            commands::preview_source::read_preview_source,
            commands::preview_source::read_preview_pair,
            commands::delete::delete_assets,
            export_plugin::assemble_glb_for_asset,
            export_plugin::perform_asset_copy,
            export_plugin::preview_export,
            export_plugin::asset_fingerprints,
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
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn not_found() -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(404)
        .body(Vec::new())
        .expect("static 404 response")
}
