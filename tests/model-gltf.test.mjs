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
const { createGltfFormat } = await import(load("../plugins/com.toybox.model-gltf/lib/gltf.ts"));

const enc = (s) => new TextEncoder().encode(s);
const dec = (b) => new TextDecoder().decode(b);

function memoryFs(files = {}) {
  const store = new Map(Object.entries(files).map(([k, v]) => [k, typeof v === "string" ? enc(v) : v]));
  return {
    store,
    async readBytes(root, path) { const b = store.get(`${root}|${path}`); if (!b) throw new Error(`missing ${path}`); return b; },
    async writeText(root, path, text) { store.set(`${root}|${path}`, enc(text)); },
    async writeBytes(root, path, bytes) { store.set(`${root}|${path}`, bytes); },
  };
}
const target = { libraryRoot: "L", pack: "pk", category: "props" };

/** A minimal binary glTF: the JSON chunk and a 4-byte BIN chunk. */
function glb(doc, bin = new Uint8Array([1, 2, 3, 4])) {
  const json = enc(JSON.stringify(doc).padEnd(Math.ceil(JSON.stringify(doc).length / 4) * 4, " "));
  const out = new Uint8Array(12 + 8 + json.length + 8 + bin.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, 0x46546c67, true); view.setUint32(4, 2, true); view.setUint32(8, out.length, true);
  view.setUint32(12, json.length, true); view.setUint32(16, 0x4e4f534a, true); out.set(json, 20);
  const at = 20 + json.length;
  view.setUint32(at, bin.length, true); view.setUint32(at + 4, 0x004e4942, true); out.set(bin, at + 8);
  return out;
}

test("the format claims gltf and glb and keeps no original", () => {
  const format = createGltfFormat(memoryFs());
  assert.deepEqual([format.id, format.label, format.extensions, format.keepsOriginal], ["gltf", "glTF", ["gltf", "glb"], false]);
});

test("a loose glTF is copied with its buffer renamed and its textures beside it", async () => {
  const doc = { asset: { version: "2.0" }, buffers: [{ uri: "geometry.bin", byteLength: 3 }], images: [{ uri: "tex/albedo.png" }, { uri: "missing.png" }, { uri: "data:image/png;base64,AA==" }] };
  const fs = memoryFs({ "S|in/crate.gltf": JSON.stringify(doc), "S|in/geometry.bin": "bin", "S|in/tex/albedo.png": "PNG" });
  const result = await createGltfFormat(fs).import({ root: "S", path: "in/crate.gltf", stem: "crate" }, target);
  assert.deepEqual(result, { file: "crate.gltf", warnings: ["missing referenced file: missing.png"] });
  const written = JSON.parse(dec(fs.store.get("L|library/pk/props/crate.gltf")));
  assert.equal(written.buffers[0].uri, "crate.bin");
  assert.equal(dec(fs.store.get("L|library/pk/props/crate.bin")), "bin");
  assert.equal(dec(fs.store.get("L|library/pk/props/tex/albedo.png")), "PNG");
});

test("a GLB is split into a loose glTF and its bin", async () => {
  const doc = { asset: { version: "2.0" }, buffers: [{ byteLength: 4 }], images: [{ uri: "wood.png" }] };
  const fs = memoryFs({ "S|boat.glb": glb(doc), "S|wood.png": "PNG" });
  const result = await createGltfFormat(fs).import({ root: "S", path: "boat.glb", stem: "boat" }, target);
  assert.deepEqual(result, { file: "boat.gltf", warnings: [] });
  assert.equal(JSON.parse(dec(fs.store.get("L|library/pk/props/boat.gltf"))).buffers[0].uri, "boat.bin");
  assert.deepEqual([...fs.store.get("L|library/pk/props/boat.bin")], [1, 2, 3, 4]);
  assert.equal(dec(fs.store.get("L|library/pk/props/wood.png")), "PNG");
});

test("files the library cannot hold are rejected with a reason and write nothing", async () => {
  const reject = async (files, path) => {
    const fs = memoryFs(files);
    const result = await createGltfFormat(fs).import({ root: "S", path, stem: "m" }, target);
    assert.equal([...fs.store.keys()].some((key) => key.startsWith("L|")), false, path);
    return result;
  };
  const two = JSON.stringify({ buffers: [{ uri: "a.bin" }, { uri: "b.bin" }] });
  assert.deepEqual(await reject({ "S|m.gltf": two }, "m.gltf"), { rejected: "multi-buffer" });
  assert.deepEqual(await reject({ "S|m.gltf": "not json" }, "m.gltf"), { rejected: "bad-gltf" });
  assert.deepEqual(await reject({ "S|m.gltf": JSON.stringify({ buffers: [{ uri: "data:application/octet-stream;base64,AAAA" }] }) }, "m.gltf"), { rejected: "bad-gltf" });
  assert.deepEqual(await reject({ "S|m.gltf": JSON.stringify({ buffers: [{ uri: "gone.bin" }] }) }, "m.gltf"), { rejected: "bad-gltf" });
  assert.deepEqual(await reject({ "S|m.glb": enc("not a glb at all") }, "m.glb"), { rejected: "not-a-glb" });
  const embedded = glb({ buffers: [{ byteLength: 4 }], images: [{ bufferView: 0 }] });
  assert.deepEqual(await reject({ "S|m.glb": embedded }, "m.glb"), { rejected: "embedded-images" });
});
