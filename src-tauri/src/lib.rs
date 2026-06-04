mod commands;
mod config;
mod domain;
mod error;
mod infra;

use commands::{catalog, export_copy, export_glb, packs, recenter, scan, thumbs};

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
        .invoke_handler(tauri::generate_handler![
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
