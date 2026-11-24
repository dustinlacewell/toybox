import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { assetTypeOf, groupSeedVariants, variantInfo } from "../packages/toybox-sdk/dist/index.js";

function moduleUrl(file, replacements = {}) {
  let { outputText } = ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  for (const [from, to] of Object.entries(replacements)) outputText = outputText.replaceAll(`from "${from}"`, `from ${JSON.stringify(to)}`);
  return `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
}
const { buildAssetTypes, defaultAssetTypes, withAssetTypesInUse, declarationProblem, disabledAssetTypes, pluginVisibility, replacementsOf, resolveExporterId, typeProperties } = await import(moduleUrl("../src/services/assetTypes.ts"));
const { pluginKinds, acceptsKind } = await import(moduleUrl("../src/services/pluginKinds.ts"));
const stubAssetUrl = `data:text/javascript,${encodeURIComponent('export const assetUrl=async(library,path)=>"asset://"+library+"/"+path;')}`;
const { resolveAssetFile } = await import(moduleUrl("../src/services/pluginAssetFiles.ts", { "./assetUrl": stubAssetUrl }));

test("the library always has the four native types, in navigation order", () => {
  const types = defaultAssetTypes();
  assert.deepEqual(types.map((type) => type.id), ["model", "material", "animation", "audio"]);
  assert.ok(types.every((type) => type.native && !type.pluginId));
  assert.equal(types.find((type) => type.id === "audio").tileAspectRatio, 2);
});

test("a plugin declares a new type and may restyle a native one", () => {
  const viewer = () => null;
  const { types, problems } = buildAssetTypes([
    { pluginId: "com.example.skies", declarations: [{ id: "skybox", label: " Skyboxes ", singular: "Skybox", icon: "icons/sky.svg", order: 50 }], implementations: { skybox: { Viewer: viewer } } },
    { pluginId: "com.example.audio", declarations: [{ id: "audio", label: "Sounds", implementation: "native" }] },
  ]);
  assert.deepEqual(problems, []);
  assert.deepEqual(types.map((type) => type.id), ["model", "material", "animation", "audio", "skybox"]);
  const sky = types.at(-1);
  assert.deepEqual({ label: sky.label, singular: sky.singular, native: sky.native, pluginId: sky.pluginId, icon: sky.icon, tile: sky.tileAspectRatio },
    { label: "Skyboxes", singular: "Skybox", native: false, pluginId: "com.example.skies", icon: "icons/sky.svg", tile: 1 });
  assert.equal(sky.implementation.Viewer, viewer);
  // A native type keeps the host's implementation and its tile shape.
  const audio = types.find((type) => type.id === "audio");
  assert.deepEqual({ label: audio.label, native: audio.native, tile: audio.tileAspectRatio, order: audio.order, implementation: audio.implementation },
    { label: "Sounds", native: true, tile: 2, order: 40, implementation: undefined });
});

test("invalid, reserved and duplicate declarations are reported and leave the library usable", () => {
  for (const id of ["Skybox", "sky box", "x", "models", "plugin", "audio", "../etc", ""]) assert.ok(declarationProblem({ id, label: "Label" }), id);
  assert.ok(declarationProblem({ id: "skybox", label: " " }));
  assert.ok(declarationProblem({ id: "skybox", label: "Skies", implementation: "native" }), "a plugin cannot claim a native implementation");
  assert.equal(declarationProblem({ id: "audio", label: "Audio", implementation: "native" }), undefined);
  assert.equal(declarationProblem({ id: "sky-box2", label: "Skies" }), undefined);
  const { types, problems } = buildAssetTypes([
    { pluginId: "b.second", declarations: [{ id: "skybox", label: "Other skies" }] },
    { pluginId: "a.first", declarations: [{ id: "skybox", label: "Skyboxes" }, { id: "Bad Id", label: "Bad" }, null] },
  ]);
  // The first plugin by id owns a type, whatever order discovery returned them in.
  assert.equal(types.find((type) => type.id === "skybox").pluginId, "a.first");
  assert.deepEqual(problems.map((problem) => problem.pluginId).sort(), ["a.first", "a.first", "b.second"]);
  assert.equal(types.length, 5);
});

test("assets of a type whose plugin is missing still get a library section", () => {
  const types = defaultAssetTypes();
  assert.equal(withAssetTypesInUse(types, ["model", "audio"]), types);
  const extended = withAssetTypesInUse(types, ["skybox", "model", "skybox"]);
  assert.equal(extended.length, 5);
  assert.deepEqual({ id: extended[4].id, label: extended[4].label, native: extended[4].native, pluginId: extended[4].pluginId }, { id: "skybox", label: "Skybox", native: false, pluginId: undefined });
});

test("a plugin works with the types it declares; exporters may accept every type", () => {
  assert.deepEqual(pluginKinds({}), ["model"]);
  assert.deepEqual(pluginKinds({ assetKinds: ["audio"] }), ["audio"]);
  assert.deepEqual(pluginKinds({ assetTypes: [{ id: "skybox" }, { id: 3 }], assetKinds: ["skybox"] }), ["skybox"]);
  assert.deepEqual(pluginKinds({ assetTypes: [{ id: "model" }, { id: "animation" }], assetKinds: ["model"] }), ["model", "animation"]);
  assert.equal(acceptsKind({ assetKinds: ["*"] }, "skybox"), true);
  assert.equal(acceptsKind({ assetKinds: ["model", "animation"] }, "skybox"), false);
  assert.equal(acceptsKind({ assetTypes: [{ id: "skybox" }] }, "skybox"), true);
});

test("plugin assets are grouped by their declared type and never merged as size or detail variants", () => {
  assert.equal(assetTypeOf({ kind: "plugin", type: "skybox" }), "skybox");
  assert.equal(assetTypeOf({ kind: "audio" }), "audio");
  assert.deepEqual(variantInfo("Sunset_4K", "skybox"), { base: "Sunset_4K", label: "Original", rank: 0, explicit: false });
  const sky = (name) => ({ kind: "plugin", type: "skybox", id: name, pack: "p", category: "skies", name, files: [`${name}.hdr`] });
  const grouped = groupSeedVariants([sky("Sunset_4K"), { id: "m", pack: "p", category: "props", file: "Tree_LOD0.gltf" }, sky("Sunset_2K")]);
  assert.deepEqual(grouped.map((entry) => entry.kind === "plugin" ? entry.name : entry.file), ["Tree_LOD0.gltf", "Sunset_4K", "Sunset_2K"]);
  assert.ok(grouped.every((entry) => !entry.variants));
});

test("a plugin can only name files that belong to the asset it was given", () => {
  const asset = { name: "Night", files: ["library/skybox/abc/px.png", "library/skybox/abc/nx.png"] };
  assert.equal(resolveAssetFile(asset), "library/skybox/abc/px.png");
  assert.equal(resolveAssetFile(asset, "nx.png"), "library/skybox/abc/nx.png");
  assert.equal(resolveAssetFile(asset, "library/skybox/abc/nx.png"), "library/skybox/abc/nx.png");
  for (const file of ["../catalog.json", "library/models/secret.bin", "abc/px.png", ""]) assert.throws(() => resolveAssetFile(asset, file), /no file named/);
  assert.throws(() => resolveAssetFile({ name: "Empty", files: [] }), /no files/);
});

test("a switched-off plugin hides its types, unless another plugin provides them", () => {
  const models = { enabled: false, assetTypes: [{ id: "model", label: "Models", implementation: "native" }, { id: "animation", label: "Animations", implementation: "native" }] };
  const skies = { enabled: false, assetTypes: [{ id: "skybox", label: "Skyboxes" }] };
  const otherModels = { assetTypes: [{ id: "model", label: "Meshes", implementation: "native" }] };
  const exporter = { enabled: false };
  assert.deepEqual(disabledAssetTypes([models, skies, exporter]), ["animation", "model", "skybox"]);
  assert.deepEqual(disabledAssetTypes([models, otherModels]), ["animation"]);
  assert.deepEqual(disabledAssetTypes([{ ...models, enabled: true }]), []);
  // A plugin held back by a dependency counts as off, though its switch is on.
  assert.deepEqual(disabledAssetTypes([{ ...models, enabled: true, blockedBy: [{ id: "core", reason: "disabled" }] }]), ["animation", "model"]);
  assert.deepEqual(disabledAssetTypes([{ ...models, enabled: true, blockedBy: [] }]), []);

  const hidden = disabledAssetTypes([models, skies]);
  const { types } = buildAssetTypes([], hidden);
  assert.deepEqual(types.map((type) => type.id), ["material", "audio"]);
  // Items of a hidden type get no stand-in section; an unknown type still does.
  assert.deepEqual(withAssetTypesInUse(types, ["model", "skybox", "font"], hidden).map((type) => type.id), ["material", "audio", "font"]);
});

test("hiding follows the plugin an item came from when several plugins share a type", () => {
  const folders = { id: "com.toybox.material-core", assetTypes: [{ id: "material", label: "Materials", implementation: "native" }] };
  const substance = { id: "com.toybox.material-sbsar", assetKinds: ["material"], ui: { importPanel: "ui/import.js" } };
  const skies = { id: "com.toybox.skybox-core", assetTypes: [{ id: "skybox", label: "Skyboxes" }] };
  const off = (plugin) => ({ ...plugin, enabled: false });

  // Everything on: nothing hidden, whoever imported it.
  const all = pluginVisibility([folders, substance, skies]);
  assert.deepEqual(all.hiddenTypes, []);
  assert.equal(all.isHidden("material"), false);
  assert.equal(all.isHidden("material", substance.id), false);

  // The generator off: only its own materials go; the section stays.
  const noSubstance = pluginVisibility([folders, off(substance), skies]);
  assert.deepEqual(noSubstance.hiddenTypes, []);
  assert.equal(noSubstance.isHidden("material", substance.id), true);
  assert.equal(noSubstance.isHidden("material", folders.id), false);
  assert.equal(noSubstance.isHidden("material"), false);

  // The folder importer off: its materials and unmarked ones go, the section
  // stays because the generator still imports materials.
  const noFolders = pluginVisibility([off(folders), substance, skies]);
  assert.deepEqual(noFolders.hiddenTypes, []);
  assert.equal(noFolders.isHidden("material"), true);
  assert.equal(noFolders.isHidden("material", folders.id), true);
  assert.equal(noFolders.isHidden("material", substance.id), false);
  assert.deepEqual(buildAssetTypes([], noFolders.hiddenTypes).types.map((type) => type.id), ["model", "material", "animation", "audio"]);

  // Both off: the section goes with them.
  const neither = pluginVisibility([off(folders), off(substance), skies]);
  assert.deepEqual(neither.hiddenTypes, ["material"]);
  assert.equal(neither.isHidden("material", substance.id), true);

  // A plugin-implemented type needs its own plugin; an item from a plugin that
  // is no longer installed falls back to the type's owner.
  const noSkies = pluginVisibility([folders, substance, off(skies)]);
  assert.deepEqual(noSkies.hiddenTypes, ["skybox"]);
  assert.equal(noSkies.isHidden("skybox", skies.id), true);
  assert.equal(all.isHidden("material", "com.example.uninstalled"), false);

  // A plugin whose dependency is missing is as off as one the user switched off.
  const blockedSubstance = { ...substance, blockedBy: [{ id: "core", reason: "missing" }] };
  assert.equal(pluginVisibility([folders, blockedSubstance, skies]).isHidden("material", substance.id), true);
  // An exporter alone does not keep a type's section.
  const exporterOnly = { id: "com.example.export", assetKinds: ["material"], ui: {} };
  assert.deepEqual(pluginVisibility([off(folders), off(substance), exporterOnly]).hiddenTypes, ["material"]);
  assert.equal(noFolders.isHidden("material", "com.example.uninstalled"), true);
});

test("a retired plugin id resolves to the plugins that replaced it", () => {
  const plugins = [
    { id: "com.toybox.model-core", replaces: ["com.toybox.gltf-importer"] },
    { id: "com.toybox.model-gltf", replaces: ["com.toybox.gltf-importer"] },
    { id: "com.toybox.animation-core", replaces: ["com.toybox.gltf-importer"] },
    { id: "com.toybox.material-sbsar" },
    { id: "com.toybox.rig-a", replaces: ["com.toybox.humanoid-rigs", "com.toybox.older-rigs"] },
  ];
  assert.deepEqual(replacementsOf(plugins, "com.toybox.gltf-importer"), ["com.toybox.animation-core", "com.toybox.model-core", "com.toybox.model-gltf"]);
  assert.deepEqual(replacementsOf(plugins, "com.toybox.older-rigs"), ["com.toybox.rig-a"]);
  // Never retired, retired without a successor, or itself current: nothing to follow.
  assert.deepEqual(replacementsOf(plugins, "com.toybox.material-sbsar"), []);
  assert.deepEqual(replacementsOf(plugins, "com.toybox.model-fbx"), []);
  assert.deepEqual(replacementsOf(plugins, "com.toybox.model-core"), []);
});

test("an item that records a retired plugin follows its replacements instead of being hidden or orphaned", () => {
  const core = { id: "com.toybox.model-core", assetTypes: [{ id: "model", implementation: "native" }], ui: { importPanel: "p.js" }, replaces: ["com.toybox.gltf-importer"] };
  const gltf = { id: "com.toybox.model-gltf", assetKinds: ["model"], replaces: ["com.toybox.gltf-importer"] };
  const animations = { id: "com.toybox.animation-core", assetTypes: [{ id: "animation", implementation: "native" }], ui: { importPanel: "p.js" }, replaces: ["com.toybox.gltf-importer"] };
  const off = (plugin) => ({ ...plugin, enabled: false });
  const old = "com.toybox.gltf-importer";

  // The retired plugin is not installed any more, yet its items are shown.
  const all = pluginVisibility([core, gltf, animations]);
  assert.equal(all.isHidden("model", old), false);
  assert.equal(all.isHidden("animation", old), false);

  // Switching all of its successors off hides what it had imported, like switching it off did.
  const none = pluginVisibility([off(core), off(gltf), off(animations)]);
  assert.equal(none.isHidden("model", old), true);
  // One successor still on keeps the items visible (the section rule still applies per type).
  assert.equal(pluginVisibility([core, off(gltf), animations]).isHidden("model", old), false);
  assert.equal(pluginVisibility([off(core), off(gltf), animations]).isHidden("animation", old), false);
  // A type whose section is gone stays hidden whoever imported its items.
  const noAnimations = pluginVisibility([core, gltf, off(animations)]);
  assert.deepEqual(noAnimations.hiddenTypes, ["animation"]);
  assert.equal(noAnimations.isHidden("animation", old), true);

  // A plugin retired without a successor falls back to its type's owner, as an unknown plugin does.
  assert.equal(all.isHidden("model", "com.toybox.model-fbx"), false);
  assert.equal(pluginVisibility([off(core), gltf]).isHidden("model", "com.toybox.model-fbx"), true);
  // A retired id still installed keeps its own switch.
  assert.equal(pluginVisibility([core, off({ id: old })]).isHidden("model", old), true);
});

test("the renamed core plugins keep the items imported under their old ids", () => {
  // The ids and the retirement table as bundled; the library records 'com.toybox.skybox' (or nothing) for skyboxes
  // and 'com.toybox.material-sbsar' for generated materials.
  const materials = { id: "com.toybox.material-core", assetTypes: [{ id: "material", label: "Materials", implementation: "native" }], ui: { importPanel: "p.js" }, replaces: ["com.toybox.material-importer"] };
  const sbsar = { id: "com.toybox.material-sbsar", assetKinds: ["material"], ui: { importPanel: "p.js" } };
  const audio = { id: "com.toybox.audio-core", assetTypes: [{ id: "audio", label: "Audio", implementation: "native" }], ui: { importPanel: "p.js" }, replaces: ["com.toybox.audio-importer"] };
  const skies = { id: "com.toybox.skybox-core", assetTypes: [{ id: "skybox", label: "Skyboxes" }], ui: { importPanel: "p.js" }, replaces: ["com.toybox.skybox"] };
  const off = (plugin) => ({ ...plugin, enabled: false });
  const all = pluginVisibility([materials, sbsar, audio, skies]);
  for (const [type, origin] of [["material", "com.toybox.material-importer"], ["audio", "com.toybox.audio-importer"], ["skybox", "com.toybox.skybox"], ["skybox", undefined], ["audio", undefined], ["material", "com.toybox.material-sbsar"]]) {
    assert.equal(all.isHidden(type, origin), false, `${type} from ${origin}`);
  }
  // Switching the new plugin off hides what the old one imported, as switching the old one off did.
  const noSkies = pluginVisibility([materials, sbsar, audio, off(skies)]);
  assert.equal(noSkies.isHidden("skybox", "com.toybox.skybox"), true);
  assert.equal(noSkies.isHidden("skybox"), true, "skyboxes with no recorded plugin belong to the plugin that declares the type");
  assert.deepEqual(noSkies.hiddenTypes, ["skybox"]);
  // Materials rendered by Substance stay visible while the Substance plugin is on; turning Materials off blocks it, so it hides them.
  const noMaterials = pluginVisibility([off(materials), { ...sbsar, blockedBy: [{ id: materials.id, reason: "disabled" }] }, audio, skies]);
  assert.equal(noMaterials.isHidden("material", "com.toybox.material-sbsar"), true);
  assert.equal(noMaterials.isHidden("material", "com.toybox.material-importer"), true);
  // The types are owned by the new ids.
  const { types } = buildAssetTypes([
    { pluginId: skies.id, declarations: skies.assetTypes }, { pluginId: audio.id, declarations: audio.assetTypes }, { pluginId: materials.id, declarations: materials.assetTypes },
  ]);
  assert.deepEqual(types.filter((type) => type.pluginId).map((type) => [type.id, type.pluginId]), [["material", "com.toybox.material-core"], ["audio", "com.toybox.audio-core"], ["skybox", "com.toybox.skybox-core"]]);
});

test("a saved export that names a retired exporter follows the one plugin that took over", () => {
  const exporters = [{ id: "com.toybox.placer" }, { id: "com.example.new", replaces: ["com.example.old"] }, { id: "com.example.a", replaces: ["com.example.split"] }, { id: "com.example.b", replaces: ["com.example.split"] }];
  assert.equal(resolveExporterId(exporters, "com.example.old"), "com.example.new");
  assert.equal(resolveExporterId(exporters, "com.toybox.placer"), "com.toybox.placer");
  // Several successors, no successor and no exporter chosen stay as they are.
  assert.equal(resolveExporterId(exporters, "com.example.split"), "com.example.split");
  assert.equal(resolveExporterId(exporters, "com.example.gone"), "com.example.gone");
  assert.equal(resolveExporterId(exporters, ""), "");
  // An installed exporter keeps its own id even when another one lists it as replaced.
  assert.equal(resolveExporterId([{ id: "x" }, { id: "y", replaces: ["x"] }], "x"), "x");
});

test("a type declares the numbers the library sorts and filters by", () => {
  const { types, problems } = buildAssetTypes([
    { pluginId: "com.example.skies", declarations: [{ id: "skybox", label: "Skyboxes", properties: [
      { id: "width", label: "Width", path: "meta.width", unit: "px" },
      { id: "exposure", label: "Exposure", path: "meta.exposure", use: ["range"] },
      { id: "bad path", label: "Nope", path: "meta.width" },
      { id: "evil", label: "Evil", path: "meta.width') OR 1=1 --" },
      { id: "width", label: "Again", path: "meta.w" },
    ] }] },
    { pluginId: "com.example.audio", declarations: [{ id: "audio", label: "Sounds", implementation: "native" }] },
  ]);
  const sky = types.find((type) => type.id === "skybox");
  assert.deepEqual(sky.properties, [
    { id: "width", label: "Width", path: "meta.width", unit: "px", sort: true, range: true },
    { id: "exposure", label: "Exposure", path: "meta.exposure", unit: undefined, sort: false, range: true },
  ]);
  assert.equal(problems.length, 3);
  assert.ok(problems.every((problem) => problem.pluginId === "com.example.skies" && problem.message.startsWith("type 'skybox':")));
  // A native type redeclared without properties keeps the host's list.
  assert.deepEqual(types.find((type) => type.id === "audio").properties.map((property) => property.path), ["audio.duration", "audio.sampleRate", "audio.channels"]);
  assert.deepEqual(typeProperties(undefined), []);
});

test("the bundled plugins declare paths of their own records", () => {
  for (const plugin of ["skybox-core", "audio-core", "animation-core", "model-core"]) {
    const manifest = JSON.parse(readFileSync(new URL(`../plugins/com.toybox.${plugin}/manifest.json`, import.meta.url), "utf8"));
    const problems = [];
    const properties = typeProperties(manifest.assetTypes[0].properties, (message) => problems.push(message));
    assert.deepEqual(problems, [], plugin);
    assert.ok(properties.length > 0, plugin);
  }
});
