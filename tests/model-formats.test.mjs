import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

function moduleUrl(file) {
  const { outputText } = ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
}
const { collectFormats, isModelFormat, normalizeExtension, extensionOf, modelExtension, lookingFor, importWithFormat, hasFormats, NO_FORMATS } =
  await import(moduleUrl("../plugins/com.toybox.model-core/lib/formats.ts"));

const format = (id, extensions, impl) => ({ id, label: id.toUpperCase(), extensions, import: impl ?? (async () => ({ file: `${id}.gltf`, warnings: [] })) });
const source = { root: "C:/src", path: "a/b.fbx", stem: "b" };
const target = { libraryRoot: "C:/lib", pack: "p", category: "props" };

test("with no contributions nothing is handled and the hint says what to do", () => {
  const table = collectFormats([]);
  assert.deepEqual(table.extensions, []);
  assert.equal(hasFormats(table), false);
  assert.equal(lookingFor(table), "No model formats are turned on. Turn on a format plugin such as glTF models in Settings → Plugins.");
  assert.equal(lookingFor(table), NO_FORMATS);
  // Not even glTF is built in: a .glb is not a model file without the plugin.
  assert.equal(modelExtension("Tree.glb", table), null);
  assert.equal(modelExtension("Tree.gltf", table), null);
});

test("contributed formats make up the scan and the hint", () => {
  const table = collectFormats([
    { pluginId: "com.toybox.model-gltf", value: format("gltf", ["gltf", "glb"]) },
    { pluginId: "com.example.obj", value: format("obj", ["obj"]) },
  ]);
  assert.equal(hasFormats(table), true);
  assert.deepEqual(table.extensions, ["obj", "gltf", "glb"]); // claimed in plugin id order
  assert.equal(lookingFor(table), "Looks for .obj, .gltf and .glb files.");
  assert.equal(lookingFor(collectFormats([{ pluginId: "a", value: format("gltf", ["gltf", "glb"]) }])), "Looks for .gltf and .glb files.");
  assert.equal(modelExtension("Tree.GLB", table), "glb");
  assert.equal(modelExtension("tree.png", table), null);
  assert.equal(modelExtension("README", table), null);
});

test("extensions are lower-cased and the dot is dropped", () => {
  assert.equal(normalizeExtension(".OBJ"), "obj");
  assert.equal(normalizeExtension(" Fbx "), "fbx");
  assert.equal(normalizeExtension(5), "");
  assert.equal(extensionOf("a.b.GLB"), "glb");
  assert.equal(extensionOf(".hidden"), "");
  const table = collectFormats([{ pluginId: "a", value: format("x", [".DAE", "Dae", ""]) }]);
  assert.deepEqual(table.extensions, ["dae"]);
});

test("the lower plugin id wins a contested extension, whatever the input order", () => {
  const first = format("one", ["fbx"]);
  const second = format("two", ["fbx", "ply"]);
  const table = collectFormats([{ pluginId: "com.z", value: second }, { pluginId: "com.a", value: first }]);
  assert.equal(table.claims.get("fbx").format, first);
  assert.equal(table.claims.get("fbx").pluginId, "com.a");
  assert.equal(table.claims.get("ply").format, second);
});

test("gltf and glb belong to whoever claims them, like any extension", () => {
  const table = collectFormats([{ pluginId: "b", value: format("x", ["glb", "stl"]) }, { pluginId: "a", value: format("gltf", ["gltf", "glb"]) }]);
  assert.deepEqual(table.extensions, ["gltf", "glb", "stl"]);
  assert.equal(table.claims.get("glb").pluginId, "a");
  assert.equal(table.claims.get("stl").pluginId, "b");
});

test("malformed contributions are ignored", () => {
  const good = format("ok", ["obj"]);
  const table = collectFormats([
    { pluginId: "a", value: null },
    { pluginId: "b", value: "obj" },
    { pluginId: "c", value: { id: "x", label: "X", extensions: ["x"] } },
    { pluginId: "d", value: { id: "", label: "X", extensions: ["x"], import() {} } },
    { pluginId: "e", value: { id: "x", label: "X", extensions: [], import() {} } },
    { pluginId: "f", value: { id: "x", label: "X", extensions: "x", import() {} } },
    { pluginId: "g", value: good },
  ]);
  assert.deepEqual(table.extensions, ["obj"]);
  assert.equal(isModelFormat(good), true);
  assert.equal(isModelFormat(undefined), false);
});

test("a successful import is passed through with its warnings", async () => {
  let seen;
  const table = collectFormats([{ pluginId: "a", value: format("fbx", ["fbx"], async (s, t) => { seen = [s, t]; return { file: "b.gltf", warnings: ["note"] }; }) }]);
  const result = await importWithFormat(table, "fbx", source, target);
  assert.deepEqual(result, { file: "b.gltf", warnings: ["note"], originals: [], pluginId: "a", keepsOriginal: true });
  assert.deepEqual(seen, [source, target]);
});

test("a rejection keeps its reason", async () => {
  const table = collectFormats([{ pluginId: "a", value: format("obj", ["obj"], async () => ({ rejected: "no faces" })) }]);
  assert.deepEqual(await importWithFormat(table, "obj", source, target), { rejected: "no faces" });
});

test("a thrown error becomes a rejection naming the format", async () => {
  const table = collectFormats([{ pluginId: "a", value: format("obj", ["obj"], async () => { throw new Error("disk full"); }) }]);
  assert.deepEqual(await importWithFormat(table, "obj", source, target), { rejected: "OBJ import failed: disk full" });
});

test("a reply with no file is a rejection, and missing warnings become empty", async () => {
  const bad = collectFormats([{ pluginId: "a", value: format("obj", ["obj"], async () => ({})) }]);
  assert.deepEqual(await importWithFormat(bad, "obj", source, target), { rejected: "OBJ import returned no file" });
  const bare = collectFormats([{ pluginId: "a", value: format("obj", ["obj"], async () => ({ file: "x.gltf" })) }]);
  assert.deepEqual(await importWithFormat(bare, "obj", source, target), { file: "x.gltf", warnings: [], originals: [], pluginId: "a", keepsOriginal: true });
});

test("an extension nobody handles is rejected", async () => {
  assert.deepEqual(await importWithFormat(collectFormats([]), "fbx", source, target), { rejected: "no importer for .fbx files" });
});

test("a format that says its output is the original is passed through as keeping none", async () => {
  const gltf = { ...format("gltf", ["gltf", "glb"]), keepsOriginal: false };
  const table = collectFormats([{ pluginId: "com.toybox.model-gltf", value: gltf }]);
  assert.deepEqual(await importWithFormat(table, "glb", source, target), { file: "gltf.gltf", warnings: [], originals: [], pluginId: "com.toybox.model-gltf", keepsOriginal: false });
});
