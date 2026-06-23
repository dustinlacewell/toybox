//! Real end-to-end FBX conversion: shells the actual FBX2glTF binary and runs the
//! converter + promote path, asserting the library invariant holds on real output.
//! Ignored by default — needs the exe + sample fbx present (paths below). Run with:
//!   cargo test --test fbx_convert_real -- --ignored --nocapture

use std::path::Path;

const EXE: &str = r"D:\tmp\fbxtest\node_modules\fbx2gltf\bin\Windows_NT\FBX2glTF.exe";
const FBX: &str = r"D:\tmp\fbxtest\sample.fbx";

#[test]
#[ignore = "needs the real FBX2glTF exe + sample fbx on disk"]
fn converts_real_fbx_to_stem_normalized_loose_gltf() {
    let work = std::env::temp_dir().join("toybox_fbx_real");
    let _ = std::fs::remove_dir_all(&work);

    // 1) Shell the real binary exactly as the app does.
    let gltf = toybox_lib::testing_convert::convert_fbx(
        Path::new(EXE),
        Path::new(FBX),
        &work,
        "Box",
    )
    .expect("convert_fbx");

    // FBX2glTF writes into a <stem>_out/ subdir; find_gltf must have located it.
    assert!(gltf.exists(), "produced gltf exists: {gltf:?}");
    assert!(
        gltf.parent().unwrap().file_name().unwrap().to_string_lossy().contains("Box"),
        "gltf is under the <stem>_out subdir"
    );

    // 2) Promote into a fake library and assert the <stem>.bin invariant.
    let lib = work.join("lib");
    let written = toybox_lib::testing_convert::promote(
        &gltf,
        &lib,
        "library/imported/props",
        "Box",
    )
    .expect("promote");

    assert_eq!(written[0], "library/imported/props/Box.gltf");
    assert!(written.contains(&"library/imported/props/Box.bin".to_string()));
    assert!(lib.join("Box.bin").exists(), "buffer renamed to Box.bin");
    assert!(!lib.join("buffer.bin").exists(), "no phantom buffer.bin");

    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(lib.join("Box.gltf")).unwrap()).unwrap();
    assert_eq!(doc["buffers"][0]["uri"], "Box.bin", "uri rewritten");

    // The bin the catalog will record (<stem>.bin) exists and is the real buffer.
    let bin_len = std::fs::metadata(lib.join("Box.bin")).unwrap().len();
    assert_eq!(bin_len, 1224, "real buffer bytes landed at Box.bin");

    let _ = std::fs::remove_dir_all(&work);
}
