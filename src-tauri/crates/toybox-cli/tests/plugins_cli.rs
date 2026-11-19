//! Plugin commands through the `tbx` binary: the compiled bundled plugins are linked into a temporary
//! home. A test is skipped, with a message, when the plugin it needs has not been built.

mod common;

use common::*;
use serde_json::json;

#[test]
fn plugins_that_only_extend_others_say_what_they_extend() {
    let world = World::new();
    let core = world.plugin_fixture("test.core");
    world.tbx(&["plugin", "install", core.to_str().unwrap()]).ok();
    let extra = world.path("fixture-extra");
    std::fs::create_dir_all(&extra).unwrap();
    std::fs::write(extra.join("manifest.json"), json!({ "id": "test.extra", "name": "More formats", "version": "1.0.0", "dependencies": { "test.core": "*" } }).to_string()).unwrap();
    world.tbx(&["plugin", "install", extra.to_str().unwrap()]).ok();
    let list = world.tbx(&["--json", "plugin", "list"]).ok().json();
    let provides = |id: &str| list["plugins"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap()["provides"].clone();
    assert_eq!(provides("test.extra"), json!(["extends Decals"]));
    assert_eq!(provides("test.core"), json!(["types", "settings"]));
    assert!(world.tbx(&["plugin", "list"]).ok().out.contains("extends Decals"));
}

#[test]
fn imported_ids_can_be_pasted_into_show() {
    let world = World::new();
    if !world.link_repo_plugin("com.toybox.audio-core") {
        return;
    }
    let (_, _) = world.library("Main");
    let sounds = world.path("sounds");
    std::fs::create_dir_all(&sounds).unwrap();
    std::fs::write(sounds.join("beep.wav"), wav(5)).unwrap();
    let ids = world.tbx(&["import", "audio", sounds.to_str().unwrap()]).ok();
    assert_eq!(ids.lines().len(), 1);
    let shown = world.tbx(&["--json", "show", &ids.lines()[0]]).ok().json();
    assert_eq!(shown["items"][0]["asset"]["id"], ids.lines()[0].as_str());
    assert_eq!(world.tbx(&["search", "--ids"]).ok().lines(), ids.lines());
    // --dry-run names the paths and imports nothing.
    let dry = world.tbx(&["--json", "import", "audio", sounds.to_str().unwrap(), "--dry-run"]).ok().json();
    assert_eq!(dry["dryRun"], true);
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "1");
}

#[test]
fn an_import_that_skips_something_says_what_and_exits_4() {
    let world = World::new();
    if !(world.link_repo_plugin("com.toybox.model-core") && world.link_repo_plugin("com.toybox.model-gltf")) {
        return;
    }
    let (_, _) = world.library("Main");
    let models = world.path("models");
    std::fs::create_dir_all(&models).unwrap();
    let scene = json!({
        "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"name": "Tri", "mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]}],
        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}],
        "buffers": [{"uri": "Tri.bin", "byteLength": 36}]
    });
    std::fs::write(models.join("Tri.gltf"), scene.to_string()).unwrap();
    std::fs::write(models.join("Tri.bin"), [0u8; 36]).unwrap();
    let odd = world.path("odd.xyz3d");
    std::fs::write(&odd, "something no plugin reads").unwrap();
    let run = world.tbx(&["import", "model", models.join("Tri.gltf").to_str().unwrap(), odd.to_str().unwrap()]).fails_with(4);
    assert_eq!(run.lines().len(), 1, "the imported item's id is printed: {}", run.out);
    assert!(run.err.contains("skipped: ") && run.err.contains("odd.xyz3d"), "{}", run.err);
    assert!(run.err.contains("error: imported 1 item, skipped 1"), "{}", run.err);
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "1", "what could be imported was imported");
    // Under --json the reasons are in the document; the status is the same.
    let json = world.tbx(&["--json", "import", "model", models.join("Tri.gltf").to_str().unwrap(), odd.to_str().unwrap()]).fails_with(4);
    let doc = json.json();
    assert!(doc["skipped"][0].as_str().unwrap().contains("odd.xyz3d"), "{doc}");
}
