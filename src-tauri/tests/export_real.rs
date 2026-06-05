//! Integration tests exercising the export domain logic against the real asset
//! library on disk. Run with `cargo test --test export_real -- --ignored`.
//!
//! These validate the two riskiest export claims end-to-end without the Tauri
//! AppHandle: (1) GLB direct assembly produces a parseable container with images
//! embedded as bufferViews, including the lone TGA transcoded to PNG; (2) the
//! copy plan's URI rewrites + texture resolution point at real files.

use std::path::{Path, PathBuf};

use toybox_lib::testing::{export_copy_plan, glb_assemble, gltf_parse, image_embed, paths};

const ROOT: &str = r"D:\code\games\assets";

fn abs(rel: &str) -> PathBuf {
    paths::abs_under_root(ROOT, rel)
}

fn read_doc(rel: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(abs(rel)).expect("read gltf");
    gltf_parse::parse(&text).expect("parse gltf")
}

#[test]
#[ignore = "requires the real asset library on disk"]
fn glb_assembly_embeds_tga_as_png() {
    // The one TGA asset.
    let gltf_rel = "library/polygon_city/environment/SM_Env_Skyline_01.gltf";
    let doc = read_doc(gltf_rel);
    let gltf_dir = "library/polygon_city/environment";

    // Find its single buffer and read it.
    let bin_uri = &gltf_parse::buffer_uris(&doc)[0].uri;
    let bin_rel = paths::resolve_uri_rel(gltf_dir, bin_uri);
    let bin = std::fs::read(abs(&bin_rel)).expect("read bin");

    // Prepare every image (the skyline references a .tga).
    let mut images = Vec::new();
    let mut saw_tga = false;
    for img in gltf_parse::image_uris(&doc) {
        let tex_rel = paths::resolve_uri_rel(gltf_dir, &img.uri);
        let file_name = Path::new(&tex_rel).file_name().unwrap().to_string_lossy().to_string();
        if file_name.to_lowercase().ends_with(".tga") {
            saw_tga = true;
        }
        let bytes = std::fs::read(abs(&tex_rel)).expect("read texture");
        let prepared = image_embed::prepare_for_glb(&file_name, bytes).expect("prepare");
        // Every embedded image must be a GLB-legal MIME.
        assert!(prepared.mime == "image/png" || prepared.mime == "image/jpeg");
        images.push(glb_assemble::EmbedImage {
            image_index: img.index,
            mime: prepared.mime,
            bytes: prepared.bytes,
        });
    }
    assert!(saw_tga, "skyline asset should reference a .tga");

    let glb = glb_assemble::assemble(&doc, &bin, images).expect("assemble");

    // Validate the GLB container: magic, version, total length, JSON parses, and
    // every image now points at a bufferView with no uri.
    assert_eq!(&glb[0..4], b"glTF");
    assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2);
    assert_eq!(u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize, glb.len());
    assert_eq!(glb.len() % 4, 0);

    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let parsed: serde_json::Value =
        serde_json::from_slice(&glb[20..20 + json_len]).expect("embedded json parses");
    assert!(parsed["buffers"][0].get("uri").is_none(), "buffer uri dropped");
    for img in parsed["images"].as_array().unwrap() {
        assert!(img.get("uri").is_none(), "image uri dropped");
        assert!(img.get("bufferView").is_some(), "image points at bufferView");
        assert_eq!(img["mimeType"], "image/png");
    }
}

#[test]
#[ignore = "requires the real asset library on disk"]
fn copy_plan_resolves_real_textures() {
    let gltf_rel = "library/polygon_city/buildings/SM_Bld_Apartment_01.gltf";
    let bin_rel = "library/polygon_city/buildings/SM_Bld_Apartment_01.bin";
    let doc = read_doc(gltf_rel);

    let plan = export_copy_plan::plan_copy(
        &doc, gltf_rel, bin_rel, "polygon_city", "buildings", "SM_Bld_Apartment_01", false,
    )
    .expect("plan");

    // Every planned texture source must exist on disk.
    assert!(!plan.textures.is_empty());
    for tex in &plan.textures {
        assert!(abs(&tex.src_rel).exists(), "texture missing: {}", tex.src_rel);
    }
    // The rewritten gltf's image uris must be local (./textures/...).
    for img in plan.gltf["images"].as_array().unwrap() {
        let uri = img["uri"].as_str().unwrap();
        assert!(uri.starts_with("./textures/"), "uri not localized: {uri}");
    }
    // The buffer uri must point at the local bin.
    assert_eq!(plan.gltf["buffers"][0]["uri"], "SM_Bld_Apartment_01.bin");
}
