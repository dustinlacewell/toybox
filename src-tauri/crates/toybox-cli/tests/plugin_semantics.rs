//! How the runtime builds a plugin's context: permissions, extensions, the host surface without a
//! window, JSON results, problems and cancellation. Uses small plugins written for the test.
mod plugin_support;

use plugin_support::plugin_runtime::{CommandOutcome, CommandRequest, ExportRequest, ImportDefaults, ImportRequest, PluginRuntime};
use plugin_support::*;
use serde_json::{json, Map, Value};
use toybox_core::error::AppError;

const PROBE_JS: &str = r#"
import { definePlugin } from "@ldlework/toybox-sdk";
export default definePlugin((ctx) => ({
  extensions: { "probe.point": ["probe-a", "probe-b"] },
  cli: {
    commands: {
      "probe perms": async (run) => {
        const out = {};
        try { await run.fs.writeText("/x", "a.txt", "hi"); out.write = "allowed"; } catch (e) { out.write = e.message; }
        try { await run.services.tools.run("engine", []); out.tools = "allowed"; } catch (e) { out.tools = e.message; }
        try { await run.host.convertToGltf("a.fbx", "p", "c", "s"); out.convert = "allowed"; } catch (e) { out.convert = e.message; }
        return out;
      },
      "probe host": async (run) => {
        const out = { id: ctx.id, library: run.library.name };
        out.root = await run.host.getLibraryRoot();
        try { await run.host.pickDirectory(); } catch (e) { out.dir = e.message; }
        try { await run.host.pickSaveFile("a"); } catch (e) { out.save = e.message; }
        out.assets = run.assets ? run.assets.map((a) => a.id) : null;
        if (run.assets && run.assets.length) {
          const id = run.assets[0].id;
          out.sync = run.host.getAsset(id)?.name;
          out.selected = run.host.getSelectedAssets().length;
          out.async = (await run.host.getAssetAsync(id))?.name;
          out.missing = (await run.host.getAssetAsync(id + "-nope")) === undefined;
          out.library = run.assets[0].library;
        }
        return out;
      },
      "probe ext": async (run) => run.extensions("probe.point").concat(run.extensions("other.point")),
      "probe bytes": async () => new Uint8Array(4),
      "probe bigint": async () => ({ a: [1, 2n] }),
      "probe print": async (run) => { run.print("one"); run.print("two"); run.log("progress"); run.warn("careful"); console.log("hello from console"); return "ignored"; },
      "probe throws": async () => { throw new Error("plugin said no"); },
      "probe cancel": async (run) => {
        let refused;
        try { await run.fs.readDir(".", "."); } catch (e) { refused = e.message; }
        return { refused, aborted: run.signal.aborted };
      },
      "probe args": async (run) => ({ args: run.args, options: run.options }),
    },
  },
}));
"#;

type Extra<'a> = (&'a str, Value, Vec<(&'a str, String)>);

fn setup_probe(extra: &[Extra]) -> Option<Env> {
    let env = setup(&["com.toybox.audio-core"])?;
    env.install(
        "com.test.probe",
        json!({ "permissions": { "fsRead": true }, "cli": { "module": "cli.js", "commands": [
            { "name": "probe", "summary": "", "commands": [
                { "name": "perms", "summary": "", "args": [] },
                { "name": "host", "summary": "", "selection": true },
                { "name": "ext", "summary": "" },
                { "name": "bytes", "summary": "" },
                { "name": "bigint", "summary": "" },
                { "name": "print", "summary": "" },
                { "name": "throws", "summary": "" },
                { "name": "cancel", "summary": "" },
                { "name": "args", "summary": "", "args": [{ "name": "file", "type": "path" }], "options": [{ "name": "level", "type": "number", "default": 3 }, { "name": "loud", "type": "flag" }] }
            ] }] } }),
        &[("cli.js", PROBE_JS)],
    );
    for (id, manifest, files) in extra {
        let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
        env.install(id, manifest.clone(), &files);
    }
    Some(env)
}

fn command(env: &Env, rt: &PluginRuntime, path: &str, asset_ids: Option<Vec<String>>) -> Result<CommandOutcome, AppError> {
    rt.run_command("com.test.probe", CommandRequest { library: env.library.clone(), path: path.into(), args: Map::new(), options: Map::new(), asset_ids })
}

#[test]
fn permissions_are_enforced_with_the_apps_messages() {
    let Some(env) = setup_probe(&[]) else { return };
    let rt = env.runtime();
    assert!(rt.problems().is_empty(), "{:?}", rt.problems());
    let result = command(&env, &rt, "probe perms", None).unwrap().result;
    assert_eq!(result["write"], "plugin 'com.test.probe' lacks permission: fsWrite");
    assert_eq!(result["tools"], "plugin 'com.test.probe' lacks permission: runTools");
    assert_eq!(result["convert"], "plugin 'com.test.probe' lacks permission: rustConvert");
}

#[test]
fn the_host_surface_without_a_window_and_selected_assets() {
    let Some(env) = setup_probe(&[]) else { return };
    let src = env.work("sounds");
    std::fs::write(src.join("tick.wav"), wav(441, 44100)).unwrap();
    let rt = env.runtime();
    let imported = rt
        .run_importer("com.toybox.audio-core", "audio", ImportRequest { library: env.library.clone(), paths: vec![path_str(&src)], options: Map::new(), defaults: ImportDefaults::default() })
        .unwrap();
    assert_eq!(imported.imported.len(), 1);

    // Without a selection: no assets, and a host without dialogs.
    let result = command(&env, &rt, "probe host", None).unwrap().result;
    assert_eq!(result["id"], "com.test.probe");
    assert_eq!(result["library"], "Test library");
    assert_eq!(result["assets"], Value::Null);
    assert!(result["root"].as_str().unwrap().replace('\\', "/").ends_with("/library"), "{result}");
    assert!(result["dir"].as_str().unwrap().contains("no dialogs"), "{result}");
    assert!(result["save"].as_str().unwrap().contains("no dialogs"), "{result}");

    // With the selection given by raw id, the plugin sees qualified ids and the library stamp.
    let result = command(&env, &rt, "probe host", Some(imported.imported.clone())).unwrap().result;
    let expected = format!("{}::{}", env.library, imported.imported[0]);
    assert_eq!(result["assets"], json!([expected]));
    assert_eq!(result["library"], json!(env.library));
    assert_eq!(result["sync"], result["async"]);
    assert_eq!(result["selected"], 1);
    assert_eq!(result["missing"], true);
    // ... and given already qualified.
    let result = command(&env, &rt, "probe host", Some(vec![expected.clone()])).unwrap().result;
    assert_eq!(result["assets"], json!([expected]));
    // An id that is not in the library fails the command clearly.
    let err = command(&env, &rt, "probe host", Some(vec!["nope".into()])).err().unwrap();
    assert!(err.to_string().contains("asset not found"), "{err}");
}

#[test]
fn extensions_are_collected_across_plugins_ordered_by_id_and_bad_ones_are_problems() {
    let cli = json!({ "cli": { "module": "cli.js" } });
    let contributes = |value: &str| vec![("cli.js", format!("export default {{ extensions: {{ \"other.point\": [\"{value}\"] }} }};"))];
    let Some(env) = setup_probe(&[
        ("com.test.zzz", cli.clone(), contributes("z-plugin")),
        ("com.test.bbb", cli.clone(), contributes("b-plugin")),
        ("com.test.bad", cli.clone(), vec![("cli.js", "export default { extensions: { 'x.point': 5 } };".into())]),
        ("com.test.throws", cli.clone(), vec![("cli.js", "throw new Error('top level failure');".into())]),
        ("com.test.imports", cli.clone(), vec![("cli.js", "import 'node:fs'; export default {};".into())]),
        ("com.test.notobject", cli.clone(), vec![("cli.js", "export default 42;".into())]),
    ]) else {
        return;
    };
    let rt = env.runtime();
    let problems: std::collections::BTreeMap<_, _> = rt.problems().iter().cloned().collect();
    assert_eq!(problems.len(), 4, "{problems:#?}");
    assert!(problems["com.test.bad"].contains("'x.point' must be an array"));
    assert!(problems["com.test.throws"].contains("top level failure"));
    assert!(problems["com.test.imports"].contains("node:fs") && problems["com.test.imports"].contains("not available outside the app window"), "{}", problems["com.test.imports"]);
    assert!(problems["com.test.notobject"].contains("must default-export"));
    // The rest loaded: the probe sees everyone's contributions, ordered by plugin id.
    let result = command(&env, &rt, "probe ext", None).unwrap().result;
    let flat: Vec<String> = result.as_array().unwrap().iter().map(|e| format!("{}:{}", e["pluginId"].as_str().unwrap(), e["value"].as_str().unwrap())).collect();
    assert_eq!(flat, ["com.test.probe:probe-a", "com.test.probe:probe-b", "com.test.bbb:b-plugin", "com.test.zzz:z-plugin"]);
}

#[test]
fn results_must_be_json_and_events_reach_the_callback() {
    let Some(env) = setup_probe(&[]) else { return };
    let rt = env.runtime();
    let err = command(&env, &rt, "probe bytes", None).err().unwrap();
    assert!(err.to_string().contains("byte buffer") && err.to_string().contains("JSON"), "{err}");
    let err = command(&env, &rt, "probe bigint", None).err().unwrap();
    assert!(err.to_string().contains("bigint") && err.to_string().contains("the result.a[1]"), "{err}");
    let err = command(&env, &rt, "probe throws", None).err().unwrap();
    assert_eq!(err.to_string(), "plugin said no");

    env.events.borrow_mut().clear();
    let outcome = command(&env, &rt, "probe print", None).unwrap();
    assert!(outcome.printed);
    assert_eq!(outcome.result, json!("ignored"));
    assert_eq!(*env.events.borrow(), ["print: one", "print: two", "log: progress", "warn: careful", "console: hello from console"]);

    // Arguments: defaults are applied (a missing option, a flag); given values are kept.
    let outcome = rt
        .run_command(
            "com.test.probe",
            CommandRequest { library: env.library.clone(), path: "probe args".into(), args: json!({ "file": "C:/a.txt" }).as_object().unwrap().clone(), options: Map::new(), asset_ids: None },
        )
        .unwrap();
    assert_eq!(outcome.result, json!({ "args": { "file": "C:/a.txt" }, "options": { "level": 3, "loud": false } }));
}

#[test]
fn cancelling_refuses_further_host_calls_and_aborts_the_signal() {
    let Some(env) = setup_probe(&[]) else { return };
    let rt = env.runtime();
    assert_eq!(command(&env, &rt, "probe cancel", None).unwrap().result["aborted"], false);
    rt.cancel_flag().store(true, std::sync::atomic::Ordering::Relaxed);
    let result = command(&env, &rt, "probe cancel", None).unwrap().result;
    assert_eq!(result, json!({ "refused": "Cancelled", "aborted": true }));
}

const EXPORTER_JS: &str = r#"
export default {
  async run(ctx, assets) {
    ctx.report.write(assets.map((a) => a.name).join());
    try { await ctx.host.getAssetUrls(assets[0].id); } catch (e) { ctx.report.warn("urls: " + e.message); }
    try { await ctx.fs.writeText(ctx.config.targetDir + "-other", "a.txt", "x"); ctx.report.warn("outside write allowed"); } catch (e) { ctx.report.warn("outside: " + e.message); }
    try { await ctx.host.assembleGlb(assets[0].id); } catch (e) { ctx.report.warn("glb: " + e.message); }
    await ctx.fs.writeText(ctx.config.targetDir, "inside/a.txt", "x");
    ctx.report.warn("config: " + JSON.stringify(ctx.config));
    ctx.report.warn("signal: " + (ctx.signal instanceof AbortSignal));
    return ctx.report.done();
  },
  async preview(ctx, assets) { return ["planned/" + assets.length]; },
};
"#;

#[test]
fn exporters_get_the_export_ctx_and_writes_stay_in_the_target() {
    let Some(env) = setup_probe(&[(
        "com.test.exporter",
        json!({ "assetKinds": ["audio"], "permissions": { "fsWrite": true, "rustExport": true },
                "cli": { "module": "cli.js", "exporter": { "name": "probe-export", "summary": "", "options": [{ "name": "mode", "type": "string", "default": "fast" }] } } }),
        vec![("cli.js", EXPORTER_JS.to_string())],
    )]) else {
        return;
    };
    let src = env.work("sounds");
    std::fs::write(src.join("tick.wav"), wav(441, 44100)).unwrap();
    let rt = env.runtime();
    assert!(rt.problems().is_empty(), "{:?}", rt.problems());
    let imported = rt
        .run_importer("com.toybox.audio-core", "audio", ImportRequest { library: env.library.clone(), paths: vec![path_str(&src)], options: Map::new(), defaults: ImportDefaults::default() })
        .unwrap();
    let target = env.work("target");
    let request = |preview_only| ExportRequest {
        library: env.library.clone(),
        asset_ids: imported.imported.clone(),
        target_dir: path_str(&target),
        preserve_structure: true,
        options: Map::new(),
        preview_only,
    };
    let planned = rt.run_exporter("com.test.exporter", request(true)).unwrap();
    assert_eq!(planned.written, ["planned/1"]);
    assert!(!target.join("inside").exists());

    let done = rt.run_exporter("com.test.exporter", request(false)).unwrap();
    assert_eq!(done.written, ["tick"]);
    let warnings = done.warnings.join("
");
    assert!(warnings.contains("urls: getAssetUrls is not available on the command line"), "{warnings}");
    assert!(warnings.contains("may only write below"), "the write outside the target is refused: {warnings}");
    assert!(!warnings.contains("outside write allowed"));
    assert!(!std::path::Path::new(&format!("{}-other", path_str(&target))).exists());
    assert!(warnings.contains("glb:"), "an audio item cannot be baked to a glb, and that is an error not a crash: {warnings}");
    assert!(warnings.contains(r#""mode":"fast""#) && warnings.contains(r#""preserveStructure":true"#) && warnings.contains("targetDir"), "config carries the options, defaults and targetDir: {warnings}");
    assert!(warnings.contains("signal: true"));
    assert_eq!(std::fs::read_to_string(target.join("inside/a.txt")).unwrap(), "x");
}
