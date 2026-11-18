//! Opt-in verification of a migrated real library and browser fixture output.
use crate::domain::catalog_model::{AssetContent, Catalog};
use crate::domain::glb_assemble;
use crate::ops::animation_preview::{choices, read_preview};
use crate::ops::export_util::{read_bin, read_gltf};
use serde_json::{json, Value};
use std::path::PathBuf;

#[test]
#[ignore = "read-only trace regression requiring TOYBOX_TEST_CATALOG"]
fn direct_preview_trace_regression() {
    let cache =
        PathBuf::from(std::env::var_os("TOYBOX_TEST_CATALOG").expect("TOYBOX_TEST_CATALOG"));
    let catalog: Catalog = serde_json::from_slice(&std::fs::read(cache).unwrap()).unwrap();
    let mut fixtures = Vec::new();
    for name in ["SimpleMilitary_Pilot_Brown", "SimpleMilitary_Pilot_White"] {
        let animation = catalog.assets.iter().find(|asset| matches!(&asset.content,AssetContent::Animation {clip,..} if clip.source_name==name)).expect(name);
        let AssetContent::Animation { clip, fileset } = &animation.content else {
            unreachable!()
        };
        let options = choices(&catalog, &animation.id).unwrap();
        let choice = options
            .iter()
            .find(|choice| choice.asset_id == clip.source_id)
            .unwrap_or(&options[0]);
        let model = catalog
            .assets
            .iter()
            .find(|asset| asset.id == choice.asset_id)
            .unwrap();
        let expected =
            read_preview(&catalog.library_root, &catalog, &animation.id, &model.id).unwrap();
        let start = std::time::Instant::now();
        for _ in 0..20 {
            let preview = crate::ops::preview_source::read_pair(
                std::path::Path::new(&catalog.library_root),
                &fileset.gltf,
                clip.index,
                &model.fileset().unwrap().gltf,
            )
            .unwrap();
            assert_eq!(
                preview, expected,
                "Live validation and resource remapping must match the original composer"
            );
        }
        println!(
            "{name}: bounded pair read + live matching + composition {:.2} ms/thumbnail",
            start.elapsed().as_secs_f64() * 1000.0 / 20.0
        );
        fixtures.push(
            json!({"animation":animation,"model":model,"choices":options,"previewDoc":expected}),
        );
    }
    if let Some(dir) = std::env::var_os("TOYBOX_TEST_ARTIFACT_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("direct-preview-trace.json"),
            serde_json::to_vec(&json!({"catalog":catalog,"fixtures":fixtures})).unwrap(),
        )
        .unwrap();
    }
}

#[test]
#[ignore = "read-only benchmark requiring TOYBOX_TEST_CATALOG"]
fn preview_source_read_cost() {
    let cache =
        PathBuf::from(std::env::var_os("TOYBOX_TEST_CATALOG").expect("TOYBOX_TEST_CATALOG"));
    let bytes = std::fs::read(&cache).unwrap();
    let catalog: Catalog = serde_json::from_slice(&bytes).unwrap();
    let asset = catalog.assets.iter().find(|asset| matches!(&asset.content, AssetContent::Animation { clip, .. } if clip.standalone)).unwrap();
    let AssetContent::Animation { clip, fileset } = &asset.content else {
        unreachable!()
    };
    let start = std::time::Instant::now();
    for _ in 0..3 {
        let parsed: Catalog = serde_json::from_value(crate::domain::catalog_migrate::upgrade(
            serde_json::from_slice(&std::fs::read(&cache).unwrap()).unwrap(),
        ))
        .unwrap();
        std::hint::black_box(parsed);
    }
    let old_ms = start.elapsed().as_secs_f64() * 1000.0 / 3.0;
    let start = std::time::Instant::now();
    for _ in 0..50 {
        std::hint::black_box(
            crate::ops::preview_source::read_source(
                std::path::Path::new(&catalog.library_root),
                &fileset.gltf,
                Some(clip.index),
            )
            .unwrap(),
        );
    }
    let new_ms = start.elapsed().as_secs_f64() * 1000.0 / 50.0;
    println!("Catalog: {} bytes; previous catalog lookup: {old_ms:.2} ms/source; direct preview source: {new_ms:.2} ms/source", bytes.len());
}

#[test]
#[ignore = "requires TOYBOX_TEST_CATALOG; optionally writes TOYBOX_TEST_ARTIFACT_DIR"]
fn independent_animation_library_and_cross_model_preview() {
    let cache =
        PathBuf::from(std::env::var_os("TOYBOX_TEST_CATALOG").expect("TOYBOX_TEST_CATALOG"));
    let catalog: Catalog = serde_json::from_slice(&std::fs::read(cache).unwrap()).unwrap();
    let root = &catalog.library_root;
    let animations: Vec<_> = catalog
        .assets
        .iter()
        .filter(|asset| matches!(asset.content, AssetContent::Animation { .. }))
        .collect();
    assert!(!animations.is_empty());
    let mut previewable = 0;
    for asset in &animations {
        let AssetContent::Animation { clip, fileset } = &asset.content else {
            unreachable!()
        };
        assert!(clip.standalone);
        assert!(fileset.textures.is_empty());
        let doc = read_gltf(root, asset).unwrap();
        for field in ["meshes", "skins", "images", "textures", "materials"] {
            assert!(doc.get(field).is_none(), "{} contains {field}", asset.name);
        }
        assert_eq!(doc["animations"].as_array().unwrap().len(), 1);
        let bin = read_bin(root, asset).unwrap();
        let glb = glb_assemble::assemble(&doc, &bin, vec![]).unwrap();
        assert_eq!(&glb[..4], b"glTF");
        if !choices(&catalog, &asset.id).unwrap().is_empty() {
            previewable += 1;
        }
    }
    let animation = animations.iter().find(|asset| matches!(&asset.content,AssetContent::Animation {clip,..} if clip.source_name == "Character_Junky_Male_01"))
        .copied().unwrap_or(animations[0]);
    let options = choices(&catalog, &animation.id).unwrap();
    let AssetContent::Animation { clip, .. } = &animation.content else {
        unreachable!()
    };
    let choice = options
        .iter()
        .find(|choice| choice.asset_id != clip.source_id)
        .expect("At least one compatible model other than the original");
    let model = catalog
        .assets
        .iter()
        .find(|asset| asset.id == choice.asset_id)
        .unwrap();
    let preview = read_preview(root, &catalog, &animation.id, &model.id).unwrap();
    assert_eq!(preview["animations"].as_array().unwrap().len(), 1);
    assert!(preview["meshes"]
        .as_array()
        .is_some_and(|meshes| !meshes.is_empty()));
    let model_doc = read_gltf(root, model).unwrap();
    assert!(model_doc.get("animations").is_none());
    assert_eq!(preview["meshes"], model_doc["meshes"]);
    let model_options = choices(&catalog, &model.id).unwrap();
    assert!(model_options
        .iter()
        .any(|choice| choice.asset_id == animation.id));
    println!(
        "{} standalone clips; {} have compatible preview models; {} models match {}",
        animations.len(),
        previewable,
        options.len(),
        animation.name
    );
    if let Some(output) = std::env::var_os("TOYBOX_TEST_ARTIFACT_DIR") {
        let output = PathBuf::from(output);
        std::fs::create_dir_all(&output).unwrap();
        let fixture: Value = json!({"root":root,"assets":[animation,model],"rigs":catalog.rigs,"animationDoc":read_gltf(root,animation).unwrap(),"modelDoc":model_doc,"previewDoc":preview,"animationChoices":options,"modelChoices":model_options});
        std::fs::write(
            output.join("real-animation.json"),
            serde_json::to_vec(&fixture).unwrap(),
        )
        .unwrap();
        std::fs::write(
            output.join("standalone-animation.glb"),
            glb_assemble::assemble(
                &read_gltf(root, animation).unwrap(),
                &read_bin(root, animation).unwrap(),
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    }
}
