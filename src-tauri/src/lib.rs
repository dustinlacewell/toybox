mod app_host;
mod commands;
// The path-based library logic lives in `toybox-core`; these aliases keep
// `crate::domain::...`, `crate::error::...` paths valid across the app crate.
use toybox_core::{domain, error};

use commands::{
    browse, catalog, export_copy, export_glb, export_plugin, import, import_convert, library, packs,
    plugins, recenter, scan, settings, thumbs,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // Build the host (restoring the registered libraries and opening the asset scope
            // to each root) and install bundled plugins into the user's plugins dir on first run.
            app_host::setup(app.handle())?;
            Ok(())
        })
        .register_uri_scheme_protocol("plugin", serve_plugin_asset)
        .invoke_handler(tauri::generate_handler![
            library::list_libraries,
            library::add_library,
            library::create_library,
            library::rename_library,
            library::remove_library,
            library::set_default_library,
            commands::cli::cli_status,
            commands::cli::cli_install,
            commands::cli::cli_uninstall,
            commands::remote::probe_remote_library,
            commands::remote::add_remote_library,
            commands::remote::change_remote_token,
            commands::remote::reconnect_remote_library,
            commands::remote::remote_cache_size,
            commands::remote::clear_remote_cache,
            commands::remote::ensure_asset_files,
            commands::remote::list_remote_tokens,
            commands::remote::create_remote_token,
            commands::remote::revoke_remote_token,
            settings::get_fbx2gltf_path,
            settings::set_fbx2gltf_path,
            settings::set_plugin_enabled,
            settings::get_saved_exports,
            settings::set_saved_exports,
            import_convert::convert_to_gltf,
            scan::scan_library,
            browse::open_library,
            browse::query_assets,
            browse::count_assets,
            browse::query_ids,
            browse::get_assets,
            browse::facet_counts,
            browse::search_tags,
            browse::type_counts,
            browse::list_collections,
            browse::elsewhere,
            browse::rig_catalog,
            browse::list_rigs,
            browse::personal_tag_assets,
            browse::pending_thumbs,
            browse::set_favorite,
            browse::set_collection,
            browse::set_preview_choice,
            browse::drop_personal_tags,
            commands::animation_preview::compatible_previews,
            commands::animation_preview::read_animation_preview,
            catalog::library_root,
            catalog::resolve_asset_path,
            catalog::thumb_dir,
            catalog::thumb_path,
            thumbs::save_thumb,
            thumbs::invalidate_thumb,
            thumbs::set_thumb_state,
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
            plugins::plugin_hash_file,
            plugins::plugin_file_size,
            plugins::plugin_copy_file,
            plugins::plugin_image_thumb,
            plugins::plugin_write_bytes,
            plugins::plugin_write_text,
            // Services for plugins that drive an external program.
            commands::plugin_services::get_plugin_settings,
            commands::plugin_services::set_plugin_setting,
            commands::plugin_services::plugin_launch_tool,
            commands::plugin_services::plugin_run_tool,
            commands::plugin_services::plugin_host_process_id,
            commands::plugin_services::plugin_local_port_open,
            commands::plugin_services::plugin_local_json,
            commands::plugin_services::plugin_scratch_dir,
            // Per-asset export primitives plugins orchestrate.
            export_plugin::read_asset_gltf,
            commands::preview_source::read_preview_source,
            commands::preview_source::read_preview_pair,
            commands::delete::delete_assets,
            commands::metadata::update_asset_metadata,
            commands::transfer::transfer_assets,
            commands::fingerprint::start_fingerprinting,
            commands::fingerprint::fingerprint_status,
            export_plugin::assemble_glb_for_asset,
            export_plugin::perform_asset_copy,
            export_plugin::preview_export,
            export_plugin::asset_fingerprints,
            export_plugin::transcode_image,
            export_plugin::placer_merge_file,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            // Programs plugins started must not outlive the app.
            if let tauri::RunEvent::Exit = event {
                use tauri::Manager;
                if let Some(host) = app.try_state::<std::sync::Arc<toybox_host::Host>>() {
                    host.tools.stop_all();
                    host.fingerprinter.stop();
                }
            }
        });
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
    use tauri::Manager;
    let root = app.try_state::<std::sync::Arc<toybox_host::Host>>()?.paths().plugins_dir();
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
