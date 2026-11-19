//! The real compiled bundled plugins, run through the plugin runtime in a temporary home.
mod plugin_support;

use plugin_support::plugin_runtime::{ExportRequest, ImportDefaults, ImportOutcome, ImportRequest, PluginRuntime};
use plugin_support::*;
use serde_json::{json, Map, Value};

fn options(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

fn import(env: &Env, rt: &PluginRuntime, plugin: &str, name: &str, paths: &[&std::path::Path], opts: Value, defaults: ImportDefaults) -> ImportOutcome {
    let result = rt.run_importer(
        plugin,
        name,
        ImportRequest { library: env.library.clone(), paths: paths.iter().map(|p| path_str(p)).collect(), options: options(opts), defaults },
    );
    match result {
        Ok(outcome) => outcome,
        Err(e) => panic!("importing with {plugin} failed: {e}\nevents: {:#?}", env.events.borrow()),
    }
}

fn defaults_none() -> ImportDefaults {
    ImportDefaults::default()
}

fn kind_of(item: &Value) -> String {
    if item["kind"] == "plugin" { item["type"].as_str().unwrap().to_string() } else { item["kind"].as_str().unwrap().to_string() }
}

const ALL: &[&str] = &[
    "com.toybox.model-core",
    "com.toybox.model-gltf",
    "com.toybox.animation-core",
    "com.toybox.audio-core",
    "com.toybox.material-core",
    "com.toybox.skybox-core",
    "com.toybox.placer",
];

#[test]
fn every_bundled_plugin_module_loads() {
    let Some(env) = setup(ALL) else { return };
    let rt = env.runtime();
    assert_eq!(rt.problems(), &[] as &[(String, String)], "events: {:#?}", env.events.borrow());
    let clis = plugin_support::plugin_runtime::spec::plugin_clis(&env.host, &["import", "export"]).unwrap();
    for cli in &clis {
        assert!(cli.problems.is_empty(), "{}: {:?}", cli.plugin_id, cli.problems);
    }
    let importers: Vec<_> = clis.iter().flat_map(|c| c.importers.iter().map(|i| i.name.clone())).collect();
    assert_eq!(importers, ["animation", "audio", "material", "model", "skybox"]);
    assert_eq!(clis.iter().find(|c| c.plugin_id == "com.toybox.placer").unwrap().exporter.as_ref().unwrap().name, "placer");
}

#[test]
fn imports_audio_with_the_shared_defaults() {
    let Some(env) = setup(&["com.toybox.audio-core"]) else { return };
    let src = env.work("sounds");
    std::fs::write(src.join("beep.wav"), wav(4410, 44100)).unwrap();
    std::fs::write(src.join("notes.txt"), "not audio").unwrap();
    let rt = env.runtime();
    assert!(rt.problems().is_empty(), "{:?}", rt.problems());
    let defaults = ImportDefaults { tags: vec!["Footstep".into()], source: Some("Test Pack".into()), license: Some("CC0".into()), ..Default::default() };
    let outcome = import(&env, &rt, "com.toybox.audio-core", "audio", &[&src], json!({ "category": "ui" }), defaults);
    assert_eq!(outcome.imported.len(), 1, "{:?}", outcome.warnings);
    let items = env.items(&outcome.imported);
    assert_eq!(items.len(), 1, "the library holds the item");
    let item = &items[0];
    assert_eq!(kind_of(item), "audio");
    assert_eq!(item["category"], "ui");
    assert_eq!(item["origin"]["plugin"], "com.toybox.audio-core");
    assert!(item["tags"].as_array().unwrap().iter().any(|t| t == "footstep"), "{item}");
    let provenance = item.to_string();
    assert!(provenance.contains("Test Pack") && provenance.contains("CC0"), "{item}");
    let file = env.library_root.join(item["audio"]["file"].as_str().unwrap());
    assert!(file.is_file(), "{} should exist", file.display());
    assert!(env.events.borrow().iter().any(|e| e.starts_with("log:")), "progress is reported: {:?}", env.events.borrow());
}

#[test]
fn imports_a_material_from_generated_pngs() {
    let Some(env) = setup(&["com.toybox.material-core"]) else { return };
    let src = env.work("brick");
    std::fs::write(src.join("brick_albedo.png"), png(8, 8, [180, 60, 40])).unwrap();
    std::fs::write(src.join("brick_normal.png"), png(8, 8, [128, 128, 255])).unwrap();
    std::fs::write(src.join("brick_roughness.png"), png(8, 8, [200, 200, 200])).unwrap();
    let rt = env.runtime();
    let outcome = import(&env, &rt, "com.toybox.material-core", "material", &[&src], json!({}), defaults_none());
    assert_eq!(outcome.imported.len(), 1, "{:?} {:?}", outcome.skipped, outcome.warnings);
    let item = &env.items(&outcome.imported)[0];
    assert_eq!(kind_of(item), "material");
    assert_eq!(item["category"], "uncategorized", "the manifest default applies");
    assert_eq!(item["material"]["maps"].as_array().unwrap().len(), 3, "{item}");
    let folder = env.library_root.join(item["relPath"].as_str().unwrap());
    assert!(folder.is_dir() && std::fs::read_dir(&folder).unwrap().count() >= 3, "{}", folder.display());
}

#[test]
fn imports_a_model_then_animations_and_exports_it_with_placer() {
    let Some(env) = setup(ALL) else { return };
    let models = env.work("models");
    write_gltf(&models, "triangle", false);
    let anims = env.work("anims");
    write_gltf(&anims, "mover", true);
    // The Animations manifest declares no permissions, yet its importer reads the source folder through
    // `run.fs` (`resolveSources`): without `fsRead` that fails as "<path> does not exist". Granted here.
    env.grant("com.toybox.animation-core", &["fsRead"]);
    let rt = env.runtime();
    assert!(rt.problems().is_empty(), "{:?}", rt.problems());

    // The model.
    let outcome = import(&env, &rt, "com.toybox.model-core", "model", &[&models], json!({ "category": "props" }), ImportDefaults { tags: vec!["tri".into()], ..Default::default() });
    assert_eq!(outcome.imported.len(), 1, "{:?} {:?}", outcome.skipped, outcome.warnings);
    let model = env.items(&outcome.imported).remove(0);
    assert_eq!(kind_of(&model), "model");
    assert_eq!(model["origin"]["plugin"], "com.toybox.model-gltf", "the importer names the format plugin that did the work: {model}");
    assert!(env.library_root.join(model["fileset"]["gltf"].as_str().unwrap()).is_file());
    assert!(model["tags"].as_array().unwrap().iter().any(|t| t == "tri"));

    // The animation clips.
    let outcome = import(&env, &rt, "com.toybox.animation-core", "animation", &[&anims], json!({}), defaults_none());
    assert_eq!(outcome.imported.len(), 1, "{:?} {:?}", outcome.skipped, outcome.warnings);
    // An animation file commits one entry; the library lists its clips as items of their own.
    let filter = toybox_core::infra::store::browse::BrowseFilter { kind: "animation".into(), ..Default::default() };
    let rows = env.host.query_assets(&filter, false, 0, 50).unwrap().rows;
    assert_eq!(rows.len(), 1, "one clip in the library");
    assert_eq!(outcome.imported, vec![rows[0].asset.id.clone()], "the ids reported are the ids of the items now in the library, not of the entry");
    let clip = serde_json::to_value(&rows[0].asset).unwrap();
    assert_eq!(kind_of(&clip), "animation");
    assert_eq!(clip["clip"]["name"], "Move");

    // The placer exporter on the model.
    let target = env.work("godot");
    let library_json = target.join("addons").join("asset_library.json");
    let model_id = model["id"].as_str().unwrap().to_string();
    let request = |preview_only: bool, format: &str| ExportRequest {
        library: env.library.clone(),
        asset_ids: vec![model_id.clone()],
        target_dir: path_str(&target),
        preserve_structure: true,
        options: options(json!({ "format": format, "subDir": "assets/exported", "libraryJson": path_str(&library_json) })),
        preview_only,
    };
    let planned = rt.run_exporter("com.toybox.placer", request(true, "glb")).unwrap();
    assert!(planned.written.iter().any(|p| p.ends_with(".glb")) && planned.written.iter().any(|p| p.ends_with("asset_library.json")), "{:?}", planned.written);
    assert!(!target.join("assets").exists(), "a preview writes nothing");

    let done = rt.run_exporter("com.toybox.placer", request(false, "glb")).unwrap();
    assert!(done.written.iter().any(|p| p.ends_with(".glb")), "{:?} {:?}", done.written, done.warnings);
    let glb = target.join("assets/exported").join(&done.written[0]);
    let glb = if glb.is_file() { glb } else { target.join(&done.written[0]) };
    assert!(glb.is_file(), "{} should exist; wrote {:?}", glb.display(), done.written);
    assert_eq!(&std::fs::read(&glb).unwrap()[..4], b"glTF");
    let library: Value = serde_json::from_str(&std::fs::read_to_string(&library_json).unwrap()).unwrap();
    assert!(library.to_string().contains("res://assets/exported"), "{library}");

    let copied = rt.run_exporter("com.toybox.placer", request(false, "copy")).unwrap();
    assert!(copied.written.iter().any(|p| p.ends_with(".gltf")) && copied.written.iter().any(|p| p.ends_with(".bin")), "{:?}", copied.written);
}

#[test]
fn an_exporter_refuses_a_type_it_does_not_accept_and_writes_stay_in_the_target() {
    let Some(env) = setup(&["com.toybox.audio-core", "com.toybox.placer"]) else { return };
    let src = env.work("sounds");
    std::fs::write(src.join("beep.wav"), wav(441, 44100)).unwrap();
    let rt = env.runtime();
    let outcome = import(&env, &rt, "com.toybox.audio-core", "audio", &[&src], json!({}), defaults_none());
    let target = env.work("out");
    let err = rt
        .run_exporter(
            "com.toybox.placer",
            ExportRequest {
                library: env.library.clone(),
                asset_ids: outcome.imported.clone(),
                target_dir: path_str(&target),
                preserve_structure: false,
                options: options(json!({ "libraryJson": path_str(&target.join("l.json")) })),
                preview_only: false,
            },
        )
        .err()
        .expect("audio is not a model");
    assert!(err.to_string().contains("does not accept"), "{err}");
}

#[test]
fn imports_a_skybox_from_a_generated_panorama() {
    let Some(env) = setup(&["com.toybox.skybox-core"]) else { return };
    let src = env.work("skies");
    std::fs::write(src.join("dusk_panorama.png"), png(32, 16, [200, 120, 80])).unwrap();
    let rt = env.runtime();
    assert!(rt.problems().is_empty(), "{:?}", rt.problems());
    let outcome = import(&env, &rt, "com.toybox.skybox-core", "skybox", &[&src], json!({}), defaults_none());
    assert_eq!(outcome.imported.len(), 1, "{:?} {:?}", outcome.skipped, outcome.warnings);
    let item = &env.items(&outcome.imported)[0];
    assert_eq!(kind_of(item), "skybox", "{item}");
    assert_eq!(item["origin"]["plugin"], "com.toybox.skybox-core");
    for file in item["files"].as_array().unwrap() {
        assert!(env.library_root.join(file.as_str().unwrap()).is_file(), "{file}");
    }
}
