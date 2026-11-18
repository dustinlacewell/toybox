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
const core = "../plugins/com.toybox.model-core/";
const { newStagedNames, planOriginal, cleanCompanions, companionPlacement } = await import(load(`${core}lib/originals.ts`));
const { collectFormats, importWithFormat } = await import(load(`${core}lib/formats.ts`));
const { materialize, keepOriginal, findModels } = await import(load(`${core}lib/walk.ts`));
const { createGltfFormat } = await import(load("../plugins/com.toybox.model-gltf/lib/gltf.ts"));
const { withOrigin } = await import(load("../src/domain/origin.ts"));
const { importedFrom } = await import(load("../src/domain/facets.ts"));

const enc = (s) => new TextEncoder().encode(s);

/** An in-memory stand-in for the host file access. */
function memoryFs(files = {}) {
  const store = new Map(Object.entries(files).map(([k, v]) => [k, typeof v === "string" ? enc(v) : v]));
  return {
    store,
    async readBytes(root, path) { const b = store.get(`${root}|${path}`); if (!b) throw new Error(`missing ${path}`); return b; },
    async writeText(root, path, text) { store.set(`${root}|${path}`, enc(text)); },
    async writeBytes(root, path, bytes) { store.set(`${root}|${path}`, bytes); },
    async copyFile(root, path, to, target) { const b = store.get(`${root}|${path}`); if (!b) throw new Error(`missing ${path}`); store.set(`${to}|${target}`, b); },
  };
}

/** A fake format that converts "thing" files and reports companions. */
function fakeFormat(fs, companionsOf = () => []) {
  return {
    id: "thing", label: "Thing", extensions: ["thing"],
    async import(source, target) {
      const dir = `library/${target.pack}/${target.category}`;
      await fs.writeText(target.libraryRoot, `${dir}/${source.stem}.gltf`, JSON.stringify({ asset: { version: "2.0" }, buffers: [{ uri: `${source.stem}.bin` }], images: [{ uri: "tex.png" }] }));
      await fs.writeBytes(target.libraryRoot, `${dir}/${source.stem}.bin`, enc("bin"));
      return { file: `${source.stem}.gltf`, warnings: [], originals: companionsOf(source) };
    },
  };
}
const found = (relPath) => ({ relPath, stem: relPath.replace(/^.*\//, "").replace(/\.[^.]+$/, ""), ext: relPath.split(".").pop() });

test("companion files pass through with the plugin that made the import, and are cleaned", async () => {
  const table = collectFormats([{ pluginId: "com.example.thing", value: fakeFormat(memoryFs(), () => ["a/x.mat", "a\\y.mat", "../out.mat", "a/x.mat", 5, "a/b.thing"]) }]);
  const result = await importWithFormat(table, "thing", { root: "S", path: "a/b.thing", stem: "b" }, { libraryRoot: "L", pack: "p", category: "c" });
  assert.equal(result.pluginId, "com.example.thing");
  assert.deepEqual(cleanCompanions(result.originals, "a/b.thing"), ["a/x.mat", "a/y.mat"]);
});

test("a format that lists no companions yields an empty list", async () => {
  const format = { ...fakeFormat(memoryFs()), import: async () => ({ file: "t.gltf", warnings: [] }) };
  const result = await importWithFormat(collectFormats([{ pluginId: "p", value: format }]), "thing", { root: "S", path: "t.thing", stem: "t" }, { libraryRoot: "L", pack: "p", category: "c" });
  assert.deepEqual(result.originals, []);
});

test("companions keep their place relative to the original, never above it", () => {
  assert.equal(companionPlacement("models/crate.obj", "models/mat/crate.mtl"), "mat/crate.mtl");
  assert.equal(companionPlacement("models/crate.obj", "shared/crate.mtl"), "crate.mtl");
  assert.equal(companionPlacement("crate.obj", "crate.mtl"), "crate.mtl");
});

test("the plan names the source first and records where each companion belongs", () => {
  const plan = planOriginal(newStagedNames(), "thing", "m/crate.thing", ["m/mat/crate.mat", "other/lib.mat"], ["crate.gltf", "crate.bin"]);
  assert.deepEqual(plan.files.map((f) => [f.from, f.name]), [["m/crate.thing", "crate.thing"], ["m/mat/crate.mat", "crate.mat"], ["other/lib.mat", "lib.mat"]]);
  assert.deepEqual(plan.origin, {
    files: ["crate.thing", "crate.mat", "lib.mat"],
    meta: { format: "thing", source: "m/crate.thing", companions: { "crate.mat": "mat/crate.mat", "lib.mat": "lib.mat" } },
  });
  assert.equal(planOriginal(newStagedNames(), "thing", "a.thing", [], ["a.gltf", "a.bin"]).origin.meta.companions, undefined);
});

test("an original never takes the name of a converted file or of another original", () => {
  const names = newStagedNames();
  const first = planOriginal(names, "thing", "a/shared.mat.thing", ["a/same.mat", "b/same.mat", "tex.png"], ["shared.mat.gltf", "shared.mat.bin", "tex.png"]);
  assert.deepEqual(first.origin.files, ["shared.mat.thing", "same.mat", "same_2.mat", "tex_2.png"]);
  // The same companion used by a second model is staged under the same name.
  const second = planOriginal(names, "thing", "a/other.thing", ["a/same.mat"], ["other.gltf", "other.bin"]);
  assert.deepEqual(second.origin.files, ["other.thing", "same.mat"]);
  // But a different file of the same name in another folder is told apart.
  const third = planOriginal(names, "thing", "c/third.thing", ["c/same.mat"], ["third.gltf", "third.bin"]);
  assert.deepEqual(third.origin.files, ["third.thing", "same_3.mat"]);
  // Case does not make names different.
  const clash = planOriginal(newStagedNames(), "thing", "crate.thing", [], ["CRATE.THING"]);
  assert.deepEqual(clash.origin.files, ["crate_2.thing"]);
});

test("importing through a format keeps the source and its companions beside the converted glTF", async () => {
  const fs = memoryFs({ "S|props/crate.thing": "SOURCE", "S|props/crate.mat": "MAT", "S|props/mats/lib.mat": "LIB" });
  const table = collectFormats([{ pluginId: "com.example.thing", value: fakeFormat(fs, () => ["props/crate.mat", "props/mats/lib.mat", "props/missing.mat"]) }]);
  const io = { fs, sourceRoot: "S", libraryRoot: "L", formats: table };
  const f = found("props/crate.thing");
  const imported = await materialize(io, f, "_import_x", "props");
  assert.equal(imported.file, "crate.gltf");
  const kept = await keepOriginal(io, f, imported, "_import_x", "props", newStagedNames());
  assert.deepEqual(kept.origin, {
    plugin: "com.example.thing",
    files: ["crate.thing", "crate.mat", "lib.mat"],
    meta: { format: "thing", source: "props/crate.thing", companions: { "crate.mat": "crate.mat", "lib.mat": "mats/lib.mat" } },
  });
  assert.equal(kept.warnings.length, 1);
  assert.match(kept.warnings[0], /props\/missing\.mat/);
  const dir = "library/_import_x/props";
  assert.equal(new TextDecoder().decode(fs.store.get(`L|${dir}/crate.thing`)), "SOURCE");
  assert.equal(new TextDecoder().decode(fs.store.get(`L|${dir}/lib.mat`)), "LIB");
  assert.ok(fs.store.has(`L|${dir}/crate.gltf`) && fs.store.has(`L|${dir}/crate.bin`));
  assert.ok(!fs.store.has(`L|${dir}/missing.mat`));
});

test("a source that cannot be copied leaves the entry without an origin", async () => {
  const fs = memoryFs({ "S|a.thing": "x" });
  const table = collectFormats([{ pluginId: "p", value: fakeFormat(fs) }]);
  const io = { fs, sourceRoot: "S", libraryRoot: "L", formats: table };
  const f = found("a.thing");
  const imported = await materialize(io, f, "pk", "c");
  fs.store.delete("S|a.thing");
  const kept = await keepOriginal(io, f, imported, "pk", "c", newStagedNames());
  assert.equal(kept.origin, undefined);
  assert.match(kept.warnings[0], /could not be kept/);
});

test("the glTF format keeps no original, only the plugin that imported it", async () => {
  const gltf = JSON.stringify({ asset: { version: "2.0" }, buffers: [{ uri: "m.bin" }] });
  const fs = memoryFs({ "S|m.gltf": gltf, "S|m.bin": "bin" });
  const formats = collectFormats([{ pluginId: "com.toybox.model-gltf", value: createGltfFormat(fs) }]);
  const io = { fs, sourceRoot: "S", libraryRoot: "L", formats };
  const f = found("m.gltf");
  const imported = await materialize(io, f, "pk", "c");
  assert.equal(imported.pluginId, "com.toybox.model-gltf");
  assert.equal(imported.keepsOriginal, false);
  const kept = await keepOriginal(io, f, imported, "pk", "c", newStagedNames());
  assert.deepEqual(kept, { origin: { plugin: "com.toybox.model-gltf" }, warnings: [] });
  // Nothing was copied beside the model but the converted files.
  assert.deepEqual([...fs.store.keys()].filter((key) => key.startsWith("L|")).sort(), ["L|library/pk/c/m.bin", "L|library/pk/c/m.gltf"]);
});

test("with no format turned on nothing is found and nothing can be imported", async () => {
  const fs = { ...memoryFs(), async readDir() { throw new Error("must not be walked"); } };
  const io = { fs, sourceRoot: "S", libraryRoot: "L", formats: collectFormats([]) };
  assert.deepEqual(await findModels(fs, "S", io.formats), []);
  assert.deepEqual(await materialize(io, found("m.gltf"), "pk", "c"), { rejected: "no importer for .gltf files" });
});

test("the host stamps the importer, but honours a plugin that depends on it", () => {
  const entry = { id: "a", pack: "p", category: "c", file: "a.gltf", origin: { plugin: "com.toybox.model-fbx", files: ["a.fbx"], meta: { format: "fbx" } },
    variants: [{ id: "b", pack: "p", category: "c", file: "a_LOD1.gltf", origin: { plugin: "com.toybox.model-fbx", files: ["a_LOD1.fbx"] } }] };
  const named = withOrigin(entry, "com.toybox.model-core", ["com.toybox.model-fbx"]);
  assert.deepEqual(named.origin, { plugin: "com.toybox.model-fbx", meta: { format: "fbx" }, files: ["a.fbx"] });
  assert.equal(named.variants[0].origin.plugin, "com.toybox.model-fbx");
  const claimed = withOrigin(entry, "com.toybox.model-core", ["com.example.other"]);
  assert.equal(claimed.origin.plugin, "com.toybox.model-core");
  assert.deepEqual(claimed.origin.files, ["a.fbx"]);
  assert.equal(withOrigin(entry, "com.toybox.model-core").origin.plugin, "com.toybox.model-core");
});

test("the properties row says what an asset was imported from and whether the original is kept", () => {
  assert.equal(importedFrom({ format: "fbx", origin: { plugin: "x", files: ["library/models/k/a.fbx"] } }), "FBX (original kept)");
  assert.equal(importedFrom({ format: "fbx", origin: { plugin: "x" } }), "FBX");
  assert.equal(importedFrom({ format: ".OBJ" }), "OBJ");
  assert.equal(importedFrom({ format: "glb", origin: { plugin: "x", files: [] } }), "GLB");
  assert.equal(importedFrom({}), null);
});
