import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

// Transpile a TypeScript file and the relative modules it imports into data: URLs.
const cache = new Map();
function load(file) {
  const url = new URL(file, import.meta.url);
  if (cache.has(url.href)) return cache.get(url.href);
  let { outputText } = ts.transpileModule(readFileSync(url, "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  outputText = outputText.replace(/from "(\.[^"]+)\.js"/g, (_, rel) => `from "${load(new URL(`${rel}.ts`, url).href)}"`);
  const out = `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
  cache.set(url.href, out);
  return out;
}
const { createModelApi } = await import(load("../plugins/com.toybox.model-core/lib/service.ts"));
const { NO_FORMATS } = await import(load("../plugins/com.toybox.model-core/lib/formats.ts"));
const { createGltfFormat } = await import(load("../plugins/com.toybox.model-gltf/lib/gltf.ts"));
const { findModelApi, isModelApi, MODEL_API, MODEL_CORE } = await import(load("../plugins/com.toybox.animation-core/lib/modelApi.ts"));
const { importAnimations } = await import(load("../plugins/com.toybox.animation-core/lib/run.ts"));
const { collectExtensions, extensionLookup, setExtensionTable } = await import(load("../src/services/pluginExtensions.ts"));

const enc = (s) => new TextEncoder().encode(s);

/** An in-memory stand-in for the host file access that records every call. */
function memoryFs(files = {}) {
  const store = new Map(Object.entries(files).map(([k, v]) => [k, typeof v === "string" ? enc(v) : v]));
  const calls = [];
  return {
    store, calls,
    async readDir(root, dir) {
      calls.push(["readDir", root, dir]);
      const prefix = dir === "." ? "" : `${dir}/`;
      const names = new Map();
      for (const key of store.keys()) {
        const [r, path] = key.split("|");
        if (r !== root || !path.startsWith(prefix)) continue;
        const rest = path.slice(prefix.length);
        const slash = rest.indexOf("/");
        names.set(slash < 0 ? rest : rest.slice(0, slash), slash >= 0);
      }
      return [...names].map(([name, isDir]) => ({ name, relPath: prefix + name, isDir }));
    },
    async readBytes(root, path) { const b = store.get(`${root}|${path}`); if (!b) throw new Error(`missing ${path}`); return b; },
    async writeText(root, path, text) { calls.push(["writeText", root, path]); store.set(`${root}|${path}`, enc(text)); },
    async writeBytes(root, path, bytes) { calls.push(["writeBytes", root, path]); store.set(`${root}|${path}`, bytes); },
    async copyFile(root, path, to, target) { calls.push(["copyFile", root, path, to, target]); },
  };
}
const gltfDoc = (animations, bin = "walk.bin") => JSON.stringify({ asset: { version: "2.0" }, buffers: [{ uri: bin, byteLength: 3 }], ...(animations ? { animations } : {}) });

test("with no format plugin on, the service reports no formats and finds nothing", async () => {
  const fs = memoryFs({ "S|walk.gltf": gltfDoc() });
  const api = createModelApi(fs, () => []);
  assert.deepEqual(api.formats(), []);
  assert.equal(api.lookingFor(), NO_FORMATS);
  assert.deepEqual(await api.scan("S"), []);
  assert.deepEqual(fs.calls, [], "the folder is not even read");
  assert.deepEqual(await api.convert("S", "L", { relPath: "walk.gltf", stem: "walk", ext: "gltf" }, "pk", "animations"), { rejected: "no importer for .gltf files" });
});

test("the service follows the format plugins that are on at the moment of each call", async () => {
  const fs = memoryFs({ "S|a/walk.gltf": gltfDoc(), "S|a/walk.bin": "bin", "S|run.fbx": "fbx" });
  let on = [];
  const api = createModelApi(fs, () => on);
  assert.deepEqual(await api.scan("S"), []);
  on = [{ pluginId: "com.toybox.model-gltf", value: createGltfFormat(fs) }];
  assert.deepEqual(api.formats(), [{ id: "gltf", label: "glTF", extensions: ["gltf", "glb"] }]);
  assert.equal(api.lookingFor(), "Looks for .gltf and .glb files.");
  const found = await api.scan("S");
  assert.deepEqual(found, [{ relPath: "a/walk.gltf", stem: "walk", ext: "gltf" }]);
  assert.deepEqual(await api.convert("S", "L", found[0], "pk", "animations"), { file: "walk.gltf", warnings: [] });
  assert.ok(fs.store.has("L|library/pk/animations/walk.gltf") && fs.store.has("L|library/pk/animations/walk.bin"));
  on = [];
  assert.deepEqual(api.formats(), []);
  assert.deepEqual(await api.scan("S"), []);
});

test("the service reads the clips of a converted file, and anything unreadable has none", async () => {
  const fs = memoryFs({
    "L|library/p/c/with.gltf": gltfDoc([{ channels: [{ sampler: 0 }], samplers: [] }]),
    "L|library/p/c/empty.gltf": gltfDoc([{ channels: [] }]),
    "L|library/p/c/none.gltf": gltfDoc(),
    "L|library/p/c/broken.gltf": "{",
  });
  const api = createModelApi(fs, () => []);
  assert.equal(await api.hasAnimationClips("L", "p", "c", "with.gltf"), true);
  for (const name of ["empty.gltf", "none.gltf", "broken.gltf", "missing.gltf"]) assert.equal(await api.hasAnimationClips("L", "p", "c", name), false, name);
});

test("a plugin's own contribution reaches a plugin that only has a panel", async () => {
  // What the host does after loading: model-core's entry module returns its service
  // under model.api, model-gltf contributes a format; animation-core has no entry module.
  const fs = memoryFs({ "S|walk.gltf": gltfDoc(), "S|walk.bin": "bin" });
  const lookup = (point) => extensionLookup(point);
  const core = { extensions: { [MODEL_API]: [createModelApi(fs, () => lookup("model.formats"))] } };
  const gltf = { extensions: { "model.formats": [createGltfFormat(fs)] } };
  const { table, problems } = collectExtensions([{ pluginId: MODEL_CORE, extensions: core.extensions }, { pluginId: "com.toybox.model-gltf", extensions: gltf.extensions }]);
  assert.deepEqual(problems, []);
  setExtensionTable(table);
  try {
    // The panel of animation-core reads it with ctx.extensions, which is this lookup.
    const api = findModelApi(extensionLookup(MODEL_API));
    assert.ok(api, "the service is delivered to its dependent");
    assert.deepEqual((await api.scan("S")).map((f) => f.relPath), ["walk.gltf"]);
    // Turn the format plugin off: the next lookup no longer sees it.
    setExtensionTable(collectExtensions([{ pluginId: MODEL_CORE, extensions: core.extensions }]).table);
    assert.deepEqual(api.formats(), []);
    // Turn the Models plugin off: there is no service to find.
    setExtensionTable(new Map());
    assert.equal(findModelApi(extensionLookup(MODEL_API)), undefined);
  } finally {
    setExtensionTable(new Map());
  }
});

test("only a well-formed service contributed by the Models plugin is used", () => {
  const api = createModelApi(memoryFs(), () => []);
  assert.equal(isModelApi(api), true);
  assert.equal(isModelApi({ ...api, scan: undefined }), false);
  assert.equal(isModelApi(null), false);
  assert.equal(findModelApi([{ pluginId: "com.example.impostor", value: api }]), undefined);
  assert.equal(findModelApi([{ pluginId: MODEL_CORE, value: { formats() {} } }]), undefined);
  assert.equal(findModelApi([{ pluginId: "com.example.impostor", value: api }, { pluginId: MODEL_CORE, value: api }]), api);
  assert.equal(findModelApi([]), undefined);
});

test("an animation import keeps no original and attaches no origin", async () => {
  const fs = memoryFs({
    "S|clips/walk.gltf": gltfDoc([{ channels: [{ sampler: 0 }] }]), "S|clips/walk.bin": "bin",
    "S|clips/still.gltf": gltfDoc(undefined, "still.bin"), "S|clips/still.bin": "bin",
    "S|other/walk.gltf": gltfDoc([{ channels: [{ sampler: 0 }] }]), "S|other/walk.bin": "bin",
    "S|clips/bad.gltf": "nope",
  });
  const api = createModelApi(fs, () => [{ pluginId: "com.toybox.model-gltf", value: createGltfFormat(fs) }]);
  const files = await api.scan("S");
  const result = await importAnimations(api, "S", "L", files.sort((a, b) => a.relPath.localeCompare(b.relPath)), "_import_x", "animations");
  assert.deepEqual(result.entries, [
    { kind: "animation", id: "uid://import__import_x_animations_animations_walk", pack: "_import_x", category: "animations", file: "walk.gltf", format: "gltf" },
  ]);
  for (const entry of result.entries) assert.deepEqual(Object.keys(entry).filter((key) => key === "origin" || key === "files"), []);
  assert.equal(fs.calls.some(([name]) => name === "copyFile"), false, "no source file is copied beside the clips");
  assert.equal(result.skipped, 3);
  assert.ok(result.warnings.some((w) => w === "skipped clips/bad.gltf: bad-gltf"));
  assert.ok(result.warnings.includes("No animation clips in clips/still.gltf"));
  assert.ok(result.warnings.includes("skipped duplicate name: other/walk.gltf"));
});
