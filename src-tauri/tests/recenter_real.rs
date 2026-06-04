//! Integration test: origin-correction against real, known-offset library
//! assets. Operates on the parsed glTF in memory (no file writes), validating
//! the world-AABB composition and recenter math on real node hierarchies.
//!
//! Run with `cargo test --test recenter_real -- --ignored`.

use capital_lib::testing_origin::{recenter, world_aabb, Align, Axis};
use capital_lib::testing::{gltf_parse, paths};

const ROOT: &str = r"D:\code\games\assets";

fn read_doc(rel: &str) -> serde_json::Value {
    let abs = paths::abs_under_root(ROOT, rel);
    let text = std::fs::read_to_string(abs).expect("read gltf");
    gltf_parse::parse(&text).expect("parse")
}

#[test]
#[ignore = "requires the real asset library on disk"]
fn chopshop_grounds_and_centers() {
    // Known offset: geometry ~5 units from origin.
    let doc = read_doc("library/polygon_scifi/buildings/SM_Bld_Chopshop_Interior_01.gltf");
    let before = world_aabb(&doc).unwrap();
    // Sanity: it really is offset from the origin.
    assert!(
        before.min[1].abs() > 0.001 || before.min[0].abs() > 0.5,
        "expected an offset asset, got {before:?}"
    );

    // Ground it: Y-min to origin.
    let (g1, bb1) = recenter(&doc, Axis::Y, Align::Min).unwrap();
    assert!(bb1.min[1].abs() < 1e-4, "base on origin, got {}", bb1.min[1]);

    // Then center X and Z on the already-grounded result.
    let (g2, bb2) = recenter(&g1, Axis::X, Align::Center).unwrap();
    let (_g3, bb3) = recenter(&g2, Axis::Z, Align::Center).unwrap();

    let cx = 0.5 * (bb3.min[0] + bb3.max[0]);
    let cz = 0.5 * (bb3.min[2] + bb3.max[2]);
    assert!(cx.abs() < 1e-4, "x centered, got {cx}");
    assert!(cz.abs() < 1e-4, "z centered, got {cz}");
    // Grounding survived the later corrections.
    assert!(bb3.min[1].abs() < 1e-4, "still grounded, got {}", bb3.min[1]);

    // The corrected doc is still valid glTF JSON with a wrapped root.
    let root = bb_root(&_g3);
    assert!(root.is_some(), "scene still has a root node");
}

#[test]
#[ignore = "requires the real asset library on disk"]
fn skinned_character_world_aabb_is_finite() {
    // A rigged character: world AABB must compose through the joint hierarchy
    // without blowing up, and recenter must work without touching the .bin.
    let doc = read_doc("library/simple_military/characters/SimpleMilitary_SpecialForces04_Black.gltf");
    let bb = world_aabb(&doc).unwrap();
    assert!(bb.min.iter().all(|v| v.is_finite()));
    assert!(bb.max.iter().all(|v| v.is_finite()));

    let (_g, grounded) = recenter(&doc, Axis::Y, Align::Min).unwrap();
    assert!(grounded.min[1].abs() < 1e-3, "feet on origin, got {}", grounded.min[1]);
}

fn bb_root(doc: &serde_json::Value) -> Option<u64> {
    doc.get("scenes")?
        .as_array()?
        .first()?
        .get("nodes")?
        .as_array()?
        .first()?
        .as_u64()
}
