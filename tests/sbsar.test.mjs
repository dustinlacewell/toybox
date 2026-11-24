import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

function moduleUrl(file) {
  const { outputText } = ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
}
const {
  readGraphs, readInputs, presetInputs, coerce, sameValue, editableInputs, presetValues, changedValues, rangeOf, toHex, fromHex,
  outputRoles, outputFile, channelOf, withVisibility, cleanName, materialName, materialId, sizeOf, readMeta, defaultMaterial,
} = await import(moduleUrl("../plugins/com.toybox.material-sbsar/lib/graph.ts"));
const { withOrigin } = await import(moduleUrl("../src/domain/origin.ts"));

// The shape the Substance engine replies with when it loads an archive.
const input = (identifier, type, guiWidget, value, extra = {}) => ({
  id: identifier.length * 1000 + type.length, identifier, label: identifier.replace(/_/g, " "), type, guiWidget, guiGroup: "", guiDescription: "",
  value, defaultValue: value, minValue: Array.isArray(value) ? value.map(() => 0) : 0, maxValue: Array.isArray(value) ? value.map(() => 0) : 1,
  sliderStep: 0, visibleIf: true, labelTrue: "True", labelFalse: "False", ...extra,
});
const output = (identifier, use = identifier) => ({ identifier, label: identifier, channelUse: [use], defaultChannelUse: use });
const preset = (label, embedded, inputs) => ({ label, embedded, value: ` <sbspreset pkgurl="pkg://tiles" label="${label}" >\n${Object.entries(inputs).map(([id, value]) => `  <presetinput identifier="${id}" uid="1" type="1" value="${value}" />`).join("\n")}\n </sbspreset>\n` });
const reply = {
  code: 0,
  data: { uuid: "ABC", graphs: [{
    index: 0, label: "Ceramic Tiles",
    inputs: [
      input("$randomseed", "integer", "NoWidget", 0),
      input("$outputsize", "integer2", "NoWidget", [9, 9]),
      input("normal_format", "integer", "Combobox", 1, { enumValues: [{ first: 0, second: "DirectX" }, { first: 1, second: "OpenGL" }] }),
      input("channel_height", "integer", "Togglebutton", 1, { guiGroup: "Channels" }),
      input("glass_color", "float3", "Color", [0.2, 0.7, 0.6]),
      input("joint_height", "float", "Slider", 0.4),
      input("glass_selection", "integer", "Combobox", 1, { maxValue: 0, enumValues: [{ first: 0, second: "Square" }, { first: 1, second: "Round" }] }),
      input("height_range", "float", "Slider", 1, { guiGroup: "Technical parameters" }),
      input("hidden_helper", "float", "NoWidget", 0.5),
    ],
    outputs: [output("glossiness"), output("diffuse"), output("normal"), output("basecolor", "baseColor"), output("height"), output("metallic"), output("roughness"), output("ambientocclusion", "ambientOcclusion"), output("specularlevel")],
    presets: [
      preset("CUSTOM", false, { $outputsize: "9,9", normal_format: "1" }),
      preset("Commander Green", true, { glass_color: "0.04,0.08,0.05", glass_selection: "0", joint_height: "0.25", $outputsize: "11,11" }),
      preset("DEFAULT", true, {}),
    ],
  }] },
};
const [graph] = readGraphs(reply);

test("an engine reply becomes a graph with the archive's own presets", () => {
  assert.equal(graph.label, "Ceramic Tiles");
  assert.equal(graph.inputs.length, 9);
  // The engine's working copy of its state is not a preset to offer.
  assert.deepEqual(graph.presets.map((item) => item.label), ["Commander Green", "DEFAULT"]);
  assert.deepEqual(graph.presets[0].inputs, { glass_color: "0.04,0.08,0.05", glass_selection: "0", joint_height: "0.25", $outputsize: "11,11" });
  assert.deepEqual(graph.inputs.find((item) => item.identifier === "glass_selection").enumValues, [{ value: 0, label: "Square" }, { value: 1, label: "Round" }]);
  assert.equal(graph.outputs.find((item) => item.identifier === "basecolor").usage, "baseColor");
  // Malformed replies never throw.
  assert.deepEqual(readGraphs(undefined), []);
  assert.deepEqual(readGraphs({ data: { graphs: "no" } }), []);
  assert.deepEqual(readInputs([{ identifier: "x" }, null, 3]), []);
  assert.deepEqual(presetInputs('<sbspreset><presetinput identifier=a value=1 /></sbspreset>'), {});
  assert.deepEqual(presetInputs('<presetinput value="2" identifier="b" />'), { b: "2" });
});

test("only the inputs a person should change are offered", () => {
  // Not the size, the normal convention, the output switches or widget-less helpers.
  assert.deepEqual(editableInputs(graph).map((item) => item.identifier), ["$randomseed", "glass_color", "joint_height", "glass_selection", "height_range"]);
});

test("a preset is the defaults with its own values on top, in the engine's types", () => {
  const defaults = presetValues(graph, null);
  assert.deepEqual(defaults, { $randomseed: 0, glass_color: [0.2, 0.7, 0.6], joint_height: 0.4, glass_selection: 1, height_range: 1 });
  const green = presetValues(graph, "Commander Green");
  assert.deepEqual(green.glass_color, [0.04, 0.08, 0.05]);
  assert.equal(green.glass_selection, 0);
  assert.equal(green.joint_height, 0.25);
  assert.equal(green.height_range, 1);
  assert.ok(!("$outputsize" in green));
  assert.deepEqual(presetValues(graph, "No such preset"), defaults);
  // Only what was changed after choosing a preset is worth recording.
  assert.deepEqual(changedValues({ ...green, joint_height: 0.3 }, green), { joint_height: 0.3 });
  assert.deepEqual(changedValues({ ...green, glass_color: [0.04, 0.08, 0.05000001] }, green), {});

  assert.deepEqual(coerce("float3", "0.5, 0.25,1"), [0.5, 0.25, 1]);
  assert.equal(coerce("integer", "3"), 3);
  assert.equal(coerce("float", " 0.75 "), 0.75);
  assert.equal(coerce("string", "hello"), "hello");
  assert.equal(coerce("float", "not a number"), "not a number");
  assert.deepEqual(coerce("integer2", [9, 9]), [9, 9]);
  assert.ok(sameValue([1, 2], [1, 2]) && !sameValue([1, 2], [1, 2, 3]) && !sameValue(1, [1]) && sameValue("a", "a") && !sameValue(undefined, 0));
});

test("an update reply changes which inputs show, never their defaults", () => {
  // After an update the engine reports the current value as the "default".
  const refreshed = readInputs(reply.data.graphs[0].inputs.map((item) => item.identifier === "joint_height" ? { ...item, value: 0.9, defaultValue: 0.9 }
    : item.identifier === "glass_color" ? { ...item, visibleIf: false } : item));
  const merged = withVisibility(graph.inputs, refreshed);
  assert.equal(merged.find((item) => item.identifier === "joint_height").defaultValue, 0.4);
  assert.equal(merged.find((item) => item.identifier === "glass_color").visibleIf, false);
  assert.equal(presetValues({ ...graph, inputs: merged }, null).joint_height, 0.4);
  // Untouched inputs keep their identity; an empty reply changes nothing.
  assert.equal(merged.find((item) => item.identifier === "height_range"), graph.inputs.find((item) => item.identifier === "height_range"));
  assert.deepEqual(withVisibility(graph.inputs, []), graph.inputs);
});

test("sliders and colours follow the archive's hints", () => {
  const byId = (id) => graph.inputs.find((item) => item.identifier === id);
  assert.deepEqual(rangeOf(byId("joint_height")), { min: 0, max: 1, step: 0.01 });
  // Substance leaves both ends at zero when it means the unit range.
  assert.deepEqual(rangeOf({ ...byId("joint_height"), minValue: 0, maxValue: 0 }), { min: 0, max: 1, step: 0.01 });
  assert.deepEqual(rangeOf({ ...byId("joint_height"), type: "integer", minValue: 1, maxValue: 8 }), { min: 1, max: 8, step: 1 });
  assert.deepEqual(rangeOf({ ...byId("joint_height"), minValue: -2, maxValue: 2, sliderStep: 0.5 }), { min: -2, max: 2, step: 0.5 });
  assert.equal(toHex([1, 0.5, 0]), "#ff8000");
  assert.equal(toHex([2, -1, 0.2]), "#ff0033");
  assert.deepEqual(fromHex("#ff8000", [0, 0, 0, 0.5]).map((part) => Math.round(part * 100) / 100), [1, 0.5, 0, 0.5]);
  assert.deepEqual(fromHex("nonsense", [0.1, 0.2, 0.3]), [0.1, 0.2, 0.3]);
});

test("outputs map onto the material's roles and the engine's file names", () => {
  assert.deepEqual(outputRoles(graph), [
    { identifier: "normal", role: "normal" }, { identifier: "basecolor", role: "albedo" }, { identifier: "height", role: "height" },
    { identifier: "metallic", role: "metallic" }, { identifier: "roughness", role: "roughness" }, { identifier: "ambientocclusion", role: "ao" },
  ]);
  // A specular/glossiness archive still gives a colour map.
  const legacy = { ...graph, outputs: graph.outputs.filter((item) => item.identifier !== "basecolor") };
  assert.deepEqual(outputRoles(legacy).find((item) => item.role === "albedo"), { identifier: "diffuse", role: "albedo" });
  assert.equal(channelOf("roughness"), "r");
  assert.equal(channelOf("albedo"), undefined);

  const outputs = ["height", "detail_height", "basecolor"];
  const files = ["toybox_Ceramic_Tiles_basecolor.png", "toybox_Ceramic_Tiles_detail_height.png", "toybox_Ceramic_Tiles_height.png", "toybox_Ceramic_Tiles_thumb.png", ".toybox"];
  assert.equal(outputFile(files, "height", outputs), "toybox_Ceramic_Tiles_height.png");
  assert.equal(outputFile(files, "detail_height", outputs), "toybox_Ceramic_Tiles_detail_height.png");
  assert.equal(outputFile(files, "basecolor", outputs), "toybox_Ceramic_Tiles_basecolor.png");
  assert.equal(outputFile(files, "normal", outputs), undefined);
});

test("names, ids and stored records are safe and stable", () => {
  assert.equal(materialName("ceramic_tiles_blue", "Commander Green"), "ceramic_tiles_blue - Commander Green");
  assert.equal(materialName("ceramic_tiles_blue", null), "ceramic_tiles_blue");
  assert.equal(cleanName('a/b:c*?"<>|.  '), "a_b_c______");
  assert.equal(cleanName("  ..  "), "Material");
  assert.equal(materialId("_import_1", "Tiles"), materialId("_import_1", "tiles"));
  assert.notEqual(materialId("_import_1", "Tiles"), materialId("_import_2", "Tiles"));
  assert.match(materialId("_import_1", "Tiles"), /^uid:\/\/sbsar_[0-9a-f]+$/);
  assert.equal(sizeOf("12"), 12);
  assert.equal(sizeOf("huge"), 11);
  assert.equal(sizeOf(40), 11);

  assert.deepEqual(readMeta({ sbsar: "tiles.sbsar", graph: 0, preset: "Green", inputs: { joint_height: 0.3, glass_color: [0, 0, 0], junk: { a: 1 } }, size: 12 }),
    { sbsar: "tiles.sbsar", graph: 0, preset: "Green", inputs: { joint_height: 0.3, glass_color: [0, 0, 0] }, size: 12 });
  assert.deepEqual(readMeta({ sbsar: "tiles.sbsar" }), { sbsar: "tiles.sbsar", graph: 0, preset: null, inputs: {}, size: 11 });
  // The fingerprint older materials recorded is no longer read: the substance (`origin.derivedFrom`) says which archive.
  assert.deepEqual(readMeta({ sbsar: "tiles.sbsar", archiveHash: "b-00ff", graphLabel: "Bricks" }), { sbsar: "tiles.sbsar", graph: 0, preset: null, inputs: {}, size: 11, graphLabel: "Bricks" });
  // A record cannot point outside the plugin's folder.
  for (const bad of [undefined, {}, { sbsar: "" }, { sbsar: "../secret.sbsar" }, { sbsar: "a\\\\b.sbsar" }, { sbsar: 3 }]) assert.equal(readMeta(bad), undefined);
});

test("the host stamps the importing plugin on every entry and its variants", () => {
  const entry = { kind: "material", id: "a", pack: "p", category: "c", name: "A", maps: [], origin: { plugin: "com.example.other", files: ["a.sbsar"], meta: { sbsar: "a.sbsar" } }, variants: [{ kind: "material", id: "b", pack: "p", category: "c", name: "A_1K", maps: [] }] };
  const stamped = withOrigin(entry, "com.toybox.material-sbsar");
  // A plugin keeps its own record but cannot claim another plugin's items.
  assert.deepEqual(stamped.origin, { plugin: "com.toybox.material-sbsar", meta: { sbsar: "a.sbsar" }, files: ["a.sbsar"] });
  assert.deepEqual(stamped.variants[0].origin, { plugin: "com.toybox.material-sbsar" });
  assert.equal(entry.origin.plugin, "com.example.other");
  assert.deepEqual(withOrigin({ kind: "audio", id: "s", pack: "p", category: "c", file: "s.wav" }, "com.toybox.audio-core").origin, { plugin: "com.toybox.audio-core" });
  assert.equal(withOrigin(entry, ""), entry);
});

test("a substance's thumbnail shows its default material", () => {
  const made = (id, name, meta, kind = "material") => ({ id, name, kind, origin: { plugin: "com.toybox.material-sbsar", ...(meta ? { meta: { sbsar: "tiles.sbsar", size: 11, ...meta } } : {}) } });
  const pristine = made("l::b", "Tiles", { graph: 0, preset: null, inputs: {} });
  const preset = made("l::a", "Tiles Green", { graph: 0, preset: "Green", inputs: {} });
  const second = made("l::c", "Tiles (Grout)", { graph: 1, preset: null, inputs: {} });
  const tweaked = made("l::d", "Tiles Wide", { graph: 0, preset: null, inputs: { width: 2 } });
  const tweakedMore = made("l::e", "Aaa", { graph: 0, preset: null, inputs: { width: 2, height: 3 } });
  assert.equal(defaultMaterial([preset, tweaked, second, pristine]).id, "l::b");
  // Without a pristine one: the fewest changed inputs, ties by name.
  assert.equal(defaultMaterial([tweakedMore, tweaked, second, preset]).id, "l::c");
  assert.equal(defaultMaterial([tweakedMore, tweaked, preset]).id, "l::a");
  assert.equal(defaultMaterial([tweakedMore, tweaked]).id, "l::d");
  // Only materials this plugin rendered count.
  assert.equal(defaultMaterial([made("l::x", "Other", undefined), made("l::y", "Sub", { graph: 0, preset: null, inputs: {} }, "substance")]), undefined);
  assert.equal(defaultMaterial([]), undefined);
});
