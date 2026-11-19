import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import ts from "typescript";

async function load(file) {
  const { outputText } = ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
}
const { isActive, isBlocked } = await load("../src/services/pluginActive.ts");
const { declaresTypes, isImporter, needsEntry, implementsTypes } = await load("../src/services/pluginCapabilities.ts");
const { blockedSentence } = await load("../src/services/pluginBlocked.ts");
const { collectExtensions, extensionLookup, setExtensionTable } = await load("../src/services/pluginExtensions.ts");
const { resolvePluginModule } = await load("../src/services/pluginModule.ts");
const { providesText, naturalList } = await load("../src/services/pluginProvides.ts");
const { pluginKinds, acceptsKind } = await load("../src/services/pluginKinds.ts");

test("a plugin is active when it is switched on and nothing it needs is unusable", () => {
  assert.equal(isActive({}), true);
  assert.equal(isActive({ enabled: true, blockedBy: [] }), true);
  assert.equal(isActive({ enabled: false }), false);
  assert.equal(isActive({ enabled: false, blockedBy: [] }), false);
  assert.equal(isActive({ enabled: true, blockedBy: [{ id: "core", reason: "disabled" }] }), false);
  // Blocked means wanted but held back; a plugin switched off is not blocked.
  assert.equal(isBlocked({ blockedBy: [{ id: "core" }] }), true);
  assert.equal(isBlocked({ enabled: false, blockedBy: [{ id: "core" }] }), false);
  assert.equal(isBlocked({}), false);
});

test("capabilities follow from what the manifest ships", () => {
  assert.deepEqual([declaresTypes({}), declaresTypes({ assetTypes: [] }), declaresTypes({ assetTypes: [{ id: "model" }] })], [false, false, true]);
  assert.deepEqual([isImporter({}), isImporter({ ui: {} }), isImporter({ ui: { importPanel: "ui/import.js" } })], [false, false, true]);
  assert.deepEqual([needsEntry({ entry: "" }), needsEntry({}), needsEntry({ entry: "index.js" })], [false, false, true]);
  assert.equal(implementsTypes({ assetTypes: [{ id: "skybox" }] }), true);
  assert.equal(implementsTypes({ assetTypes: [{ id: "model", implementation: "native" }] }), false);
  assert.equal(implementsTypes({}), false);
});

test("a blocked plugin gets one sentence per dependency, naming installed ones", () => {
  const installed = [{ id: "com.toybox.model-core", name: "Models" }];
  const core = "com.toybox.model-core";
  assert.equal(blockedSentence([{ id: core, reason: "disabled" }], installed), "Needs Models, which is turned off.");
  assert.equal(blockedSentence([{ id: "com.example.core", reason: "missing" }], installed), "Needs com.example.core, which is not installed.");
  assert.equal(blockedSentence([{ id: core, reason: "version", needs: "^2.0.0", found: "1.1.0" }], installed),
    "Needs Models 2.0.0 or compatible (^2.0.0); version 1.1.0 is installed.");
  assert.equal(blockedSentence([{ id: core, reason: "version", needs: ">=2 <3", found: "1.1.0" }], installed), "Needs Models >=2 <3; version 1.1.0 is installed.");
  assert.equal(blockedSentence([{ id: core, reason: "blocked" }], installed), "Needs Models, which cannot load.");
  assert.equal(blockedSentence([{ id: core, reason: "cycle" }], installed), "Needs Models, and Models needs it back.");
  assert.equal(blockedSentence([{ id: core, reason: "disabled" }, { id: "x", reason: "missing" }], installed), "Needs Models, which is turned off. Needs x, which is not installed.");
});

test("extensions are collected per point, ordered by plugin id then contribution order", () => {
  const { table, problems } = collectExtensions([
    { pluginId: "b.obj", extensions: { "model.formats": [{ ext: "obj" }, { ext: "mtl" }] } },
    { pluginId: "a.fbx", extensions: { "model.formats": [{ ext: "fbx" }], "other.point": ["x"] } },
    { pluginId: "c.none", extensions: undefined },
    { pluginId: "d.empty", extensions: { "model.formats": [] } },
  ]);
  assert.deepEqual(problems, []);
  assert.deepEqual(table.get("model.formats"), [
    { pluginId: "a.fbx", value: { ext: "fbx" } }, { pluginId: "b.obj", value: { ext: "obj" } }, { pluginId: "b.obj", value: { ext: "mtl" } }]);
  assert.deepEqual(table.get("other.point"), [{ pluginId: "a.fbx", value: "x" }]);
  assert.equal(table.has("d.empty"), false);
});

test("a contribution of the wrong shape is reported for its plugin and leaves the others alone", () => {
  const { table, problems } = collectExtensions([
    { pluginId: "a.bad", extensions: { "model.formats": { ext: "fbx" }, fine: [1] } },
    { pluginId: "b.worse", extensions: [1, 2] },
    { pluginId: "c.good", extensions: { "model.formats": [{ ext: "obj" }] } },
  ]);
  assert.deepEqual(problems.map((problem) => problem.pluginId), ["a.bad", "b.worse"]);
  assert.match(problems[0].message, /'model\.formats' must be an array/);
  assert.deepEqual(table.get("model.formats"), [{ pluginId: "c.good", value: { ext: "obj" } }]);
  assert.deepEqual(table.get("fine"), [{ pluginId: "a.bad", value: 1 }]);
});

test("the lookup handed out once always reads the latest table", () => {
  const lookup = extensionLookup;
  assert.deepEqual(lookup("model.formats"), []);
  setExtensionTable(collectExtensions([{ pluginId: "a", extensions: { "model.formats": [1] } }]).table);
  assert.deepEqual(lookup("model.formats"), [{ pluginId: "a", value: 1 }]);
  // Callers cannot change the stored list.
  lookup("model.formats").pop();
  assert.equal(lookup("model.formats").length, 1);
  setExtensionTable(collectExtensions([]).table);
  assert.deepEqual(lookup("model.formats"), []);
});

test("an entry module is an object or a function of the plugin context, possibly async", async () => {
  const ctx = { id: "p" };
  const mod = { run: async () => ({}) };
  assert.equal(await resolvePluginModule("p", mod, ctx), mod);
  assert.deepEqual(await resolvePluginModule("p", (given) => ({ profiles: [given.id] }), ctx), { profiles: ["p"] });
  assert.deepEqual(await resolvePluginModule("p", async (given) => ({ extensions: { point: [given.id] } }), ctx), { extensions: { point: ["p"] } });
  assert.deepEqual(await resolvePluginModule("p", {}, ctx), {});
  for (const bad of [undefined, null, 3, "x", [], () => 3, () => [], async () => null]) {
    await assert.rejects(resolvePluginModule("p", bad, ctx), /must default-export a module object/);
  }
  await assert.rejects(resolvePluginModule("p", { run: 1 }, ctx), /run must be a function/);
  await assert.rejects(resolvePluginModule("p", { profiles: {} }, ctx), /profiles must be an array/);
  await assert.rejects(resolvePluginModule("p", { types: [] }, ctx), /types must be an object/);
  await assert.rejects(resolvePluginModule("p", () => { throw new Error("boom"); }, ctx), /boom/);
});

test("the provides line names types first, then what else the plugin does", () => {
  const label = (id) => ({ model: "Models", animation: "Animations", material: "Materials" })[id] ?? id;
  assert.equal(naturalList(["A"]), "A");
  assert.equal(naturalList(["A", "B", "C"]), "A, B and C");
  const models = { assetTypes: [{ id: "model", label: "Models" }, { id: "animation" }], assetKinds: ["model", "animation"], entry: "index.js" };
  assert.equal(providesText(models, label, []), "Models and Animations");
  assert.equal(providesText(models, label, [], { exporter: true, rigProfiles: true, extensionPoints: [] }), "Models and Animations, Exporter, Rig profiles");
  const importer = { assetKinds: ["material"], ui: { importPanel: "ui/import.js" } };
  assert.equal(providesText(importer, label, []), "Imports materials");
  const fbx = { assetKinds: ["model"], entry: "index.js", ui: { importPanel: "ui/import.js" }, dependencies: { core: "^1.0.0" } };
  assert.equal(providesText(fbx, label, ["Models"], { exporter: false, rigProfiles: false, extensionPoints: ["model.formats"] }), "Imports models, Extends Models");
  // A loaded plugin that contributed nothing does not claim to extend anything.
  assert.equal(providesText(fbx, label, ["Models"], { exporter: false, rigProfiles: false, extensionPoints: [] }), "Imports models");
  // A plugin that is off is described by its manifest alone.
  assert.equal(providesText(fbx, label, ["Models"]), "Imports models, Extends Models");
  assert.equal(providesText({}, label, []), "Plugin");
});

test("a plugin’s types are its listed types plus declared ones the list leaves out", () => {
  assert.deepEqual(pluginKinds({}), ["model"]);
  assert.deepEqual(pluginKinds({ assetKinds: [] }), ["model"]);
  assert.deepEqual(pluginKinds({ assetKinds: ["audio"] }), ["audio"]);
  assert.deepEqual(pluginKinds({ assetTypes: [{ id: "skybox" }] }), ["skybox"]);
  assert.deepEqual(pluginKinds({ assetKinds: ["model"], assetTypes: [{ id: "model" }, { id: "animation" }, { id: 4 }] }), ["model", "animation"]);
  assert.equal(acceptsKind({ assetKinds: ["*"] }, "anything"), true);
  assert.equal(acceptsKind({ assetKinds: ["model"] }, "audio"), false);
});

const manifest = (id) => JSON.parse(readFileSync(new URL(`../plugins/${id}/manifest.json`, import.meta.url), "utf8"));

test("each model plugin has one responsibility, shown by what its manifest ships", () => {
  const core = manifest("com.toybox.model-core");
  const gltf = manifest("com.toybox.model-gltf");
  const animations = manifest("com.toybox.animation-core");

  // Models: the model type, its importer and the entry module that publishes model.api.
  assert.deepEqual(core.assetTypes.map((type) => type.id), ["model"]);
  assert.deepEqual([isImporter(core), needsEntry(core), core.dependencies], [true, true, undefined]);
  assert.deepEqual(pluginKinds(core), ["model"]);
  assert.equal(core.version, "3.0.0");

  // glTF models: a format only. No type, no panel, and it needs Models.
  assert.deepEqual([declaresTypes(gltf), isImporter(gltf), needsEntry(gltf)], [false, false, true]);
  assert.deepEqual(gltf.dependencies, { "com.toybox.model-core": "^3.0.0" });
  assert.deepEqual(pluginKinds(gltf), ["model"]);
  assert.equal(gltf.name, "glTF models");

  // Animations: the animation type and its panel, which reads model.api; no entry module.
  assert.deepEqual(animations.assetTypes.map((type) => [type.id, type.icon]), [["animation", "icons/animation.svg"]]);
  assert.deepEqual([isImporter(animations), needsEntry(animations)], [true, false]);
  assert.deepEqual(animations.dependencies, { "com.toybox.model-core": "^3.0.0" });
  assert.deepEqual(pluginKinds(animations), ["animation"]);
  assert.equal(acceptsKind(animations, "model"), false);
  assert.equal(acceptsKind(core, "animation"), false);
  assert.deepEqual(animations.permissions, { fsRead: true }, "it writes files only through the Models plugin; fsRead is for its command-line importer, which names files by path");

  // The dependency ranges accept the version of Models that ships.
  for (const dependent of [gltf, animations]) assert.equal(dependent.dependencies["com.toybox.model-core"].replace("^", "").split(".")[0], core.version.split(".")[0]);
  assert.equal(existsSync(new URL("../plugins/com.toybox.animation-core/icons/animation.svg", import.meta.url)), true);
  assert.equal(existsSync(new URL("../plugins/com.toybox.model-core/icons/animation.svg", import.meta.url)), false);

  // What Settings says each one provides.
  const label = (id) => ({ model: "Models", animation: "Animations" })[id] ?? id;
  assert.equal(providesText(core, label, [], { exporter: false, rigProfiles: false, extensionPoints: ["model.api"] }), "Models");
  assert.equal(providesText(gltf, label, ["Models"], { exporter: false, rigProfiles: false, extensionPoints: ["model.formats"] }), "Extends Models");
  assert.equal(providesText(animations, label, ["Models"]), "Animations");
});

test("the retired FBX and OBJ plugins are gone from the repository and from the bundle", () => {
  for (const id of ["com.toybox.model-fbx", "com.toybox.model-obj"]) {
    assert.equal(existsSync(new URL(`../plugins/${id}`, import.meta.url)), false, id);
    assert.equal(existsSync(new URL(`./${id.replace("com.toybox.", "")}.test.mjs`, import.meta.url)), false, id);
  }
  const conf = readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8");
  const build = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).scripts["plugins:build"];
  for (const id of ["com.toybox.model-core", "com.toybox.model-gltf", "com.toybox.animation-core"]) {
    for (const file of ["manifest.json", "dist", "icons"]) assert.match(conf, new RegExp(`"\.\./plugins/${id}/${file}"`), `${id}/${file}`);
    assert.match(build, new RegExp(`plugins/${id}/tsconfig.json`), id);
  }
  assert.doesNotMatch(conf, /model-(fbx|obj)/);
  assert.doesNotMatch(build, /model-(fbx|obj)/);
});
