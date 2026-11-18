import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";

const root = new URL("../", import.meta.url);
const read = (path) => readFileSync(new URL(path, root), "utf8");
const json = (path) => JSON.parse(read(path));

const rust = read("src-tauri/src/infra/bundled.rs");
const bundled = [...rust.match(/const BUNDLED_PLUGINS: &\[&str\] = &\[([\s\S]*?)\];/)[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
const retired = Object.fromEntries([...rust.match(/pub const RETIRED_PLUGINS: &\[Retirement\] = &\[([\s\S]*?)\n\];/)[1].matchAll(/\("([^"]+)", &\[([^\]]*)\]\)/g)]
  .map((match) => [match[1], [...match[2].matchAll(/"([^"]+)"/g)].map((id) => id[1])]));
const manifest = (id) => json(`plugins/${id}/manifest.json`);
const folders = readdirSync(new URL("plugins/", root), { withFileTypes: true }).filter((entry) => entry.isDirectory()).map((entry) => entry.name);

/** Does `version` satisfy `range`? Only what the bundled plugins use: `*`, `^x.y.z`, `x.y.z`. */
function satisfies(version, range) {
  if (range === "*") return true;
  const [major, minor, patch] = version.split(".").map(Number);
  const [rmajor, rminor, rpatch] = range.replace(/^\^/, "").split(".").map(Number);
  if (!range.startsWith("^")) return version === range;
  if (major !== rmajor) return false;
  if (major > 0) return minor > rminor || (minor === rminor && patch >= rpatch);
  return minor === rminor && patch >= rpatch;
}

test("the bundled plugins are every plugin folder except the dev-only placer", () => {
  assert.deepEqual([...bundled].sort(), [
    "com.toybox.animation-core", "com.toybox.audio-core", "com.toybox.material-core", "com.toybox.model-core", "com.toybox.model-gltf",
    "com.toybox.rig-quaternius", "com.toybox.rig-synty-polygon", "com.toybox.rig-synty-sidekick", "com.toybox.rig-synty-unreal", "com.toybox.skybox-core",
  ]);
  assert.deepEqual(folders.sort(), [...bundled, "com.toybox.placer"].sort(), "every plugin folder is bundled, except the placer");
});

test("every bundled manifest id is its folder name and its package matches", () => {
  for (const id of bundled) {
    const plugin = manifest(id);
    assert.equal(plugin.id, id);
    const pkg = json(`plugins/${id}/package.json`);
    assert.equal(pkg.name, "toybox-plugin-" + id.replace("com.toybox.", ""), id);
    assert.equal(pkg.version, plugin.version, `${id}: package.json and manifest agree on the version`);
    if (plugin.entry) assert.ok(existsSync(new URL(`plugins/${id}/${plugin.entry.replace(/^dist\//, "").replace(/\.js$/, ".ts")}`, root)), `${id} has the source of its entry`);
  }
});

test("every dependency of a bundled plugin is bundled at a satisfying version", () => {
  for (const id of bundled) {
    for (const [needed, range] of Object.entries(manifest(id).dependencies ?? {})) {
      assert.ok(bundled.includes(needed), `${id} needs ${needed}, which is not bundled`);
      assert.ok(satisfies(manifest(needed).version, range), `${id} needs ${needed} ${range}, bundled ${manifest(needed).version}`);
    }
  }
  assert.deepEqual(manifest("com.toybox.model-gltf").dependencies, { "com.toybox.model-core": "^3.0.0" });
});

test("the cores that were renamed start a new major version", () => {
  for (const [id, major] of [["com.toybox.model-core", 3], ["com.toybox.material-core", 2], ["com.toybox.audio-core", 2], ["com.toybox.skybox-core", 2]]) {
    assert.equal(Number(manifest(id).version.split(".")[0]), major, id);
  }
  assert.deepEqual(["material-core", "audio-core", "skybox-core"].map((name) => manifest("com.toybox." + name).name), ["Materials", "Audio", "Skyboxes"]);
});

test("no two bundled plugins declare the same type", () => {
  const owners = bundled.flatMap((id) => (manifest(id).assetTypes ?? []).map((type) => [type.id, id]));
  assert.equal(new Set(owners.map(([type]) => type)).size, owners.length);
  assert.deepEqual(owners.map(([type, id]) => `${type}:${id}`).sort(), [
    "animation:com.toybox.animation-core", "audio:com.toybox.audio-core", "material:com.toybox.material-core", "model:com.toybox.model-core", "skybox:com.toybox.skybox-core",
  ]);
});

test("the retirement table sends every old id to bundled plugins and leaves no old folder behind", () => {
  assert.deepEqual(Object.keys(retired).sort(), [
    "com.toybox.audio-importer", "com.toybox.gltf-importer", "com.toybox.humanoid-rigs", "com.toybox.material-importer", "com.toybox.model-fbx", "com.toybox.model-obj", "com.toybox.skybox",
  ]);
  for (const [old, replacements] of Object.entries(retired)) {
    assert.ok(!bundled.includes(old) && !folders.includes(old), `${old} is retired`);
    for (const id of replacements) assert.ok(bundled.includes(id), `${id} replaces ${old}`);
  }
  assert.deepEqual(retired["com.toybox.material-importer"], ["com.toybox.material-core"]);
  assert.deepEqual(retired["com.toybox.audio-importer"], ["com.toybox.audio-core"]);
  assert.deepEqual(retired["com.toybox.skybox"], ["com.toybox.skybox-core"]);
  assert.deepEqual([...retired["com.toybox.humanoid-rigs"]].sort(), bundled.filter((id) => id.includes(".rig-")).sort());
});

test("tauri.conf.json ships exactly the bundled plugins", () => {
  const resources = json("src-tauri/tauri.conf.json").bundle.resources;
  const expected = bundled.flatMap((id) => ["icons", "manifest.json", "dist"].map((part) => [`../plugins/${id}/${part}`, `plugins/${id}/${part}`]));
  assert.deepEqual(Object.entries(resources).sort(), expected.sort());
});

test("plugins:build builds the SDK first and every plugin after the ones it needs", () => {
  const steps = json("package.json").scripts["plugins:build"].split(" && ");
  assert.equal(steps[0], "pnpm sdk:build");
  const built = steps.slice(1).map((step) => step.match(/^tsc -p plugins\/([^/]+)\/tsconfig\.json$/)?.[1]);
  assert.deepEqual([...built].sort(), [...bundled, "com.toybox.placer"].sort());
  for (const id of bundled) for (const needed of Object.keys(manifest(id).dependencies ?? {})) {
    assert.ok(built.indexOf(needed) < built.indexOf(id), `${needed} is built before ${id}`);
  }
});

test("pnpm-lock.yaml has an importer block for each plugin and none for a folder that is gone", () => {
  const lock = read("pnpm-lock.yaml");
  const importers = [...lock.matchAll(/^ {2}plugins\/([^:\r\n]+):/gm)].map((match) => match[1]);
  assert.deepEqual(importers.sort(), [...folders].sort());
});

test("the plugin table in docs/plugins.md lists every plugin", () => {
  const docs = read("docs/plugins.md");
  for (const id of [...bundled, "com.toybox.placer"]) assert.ok(docs.includes("`" + id + "`"), `${id} is in docs/plugins.md`);
  for (const old of Object.keys(retired)) assert.ok(docs.includes("`" + old + "`"), `${old} is in the retirement table of docs/plugins.md`);
});
