// `tbx import sbsar` and `tbx sbsar ...` against a fake Substance engine: the
// plugin's real command-line module and engine client, a fake program behind
// `services.local`, real temp folders behind `fs`, and a pretend library that
// moves staged folders into place and replaces items of the same id as the host
// does. Run `tsc -p plugins/com.toybox.material-sbsar/tsconfig.json` first.

import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { defaultServices, listTree, loadPlugins, pluginDir, tempDir, writeTree } from "./support/cli-host.mjs";

const ID = "com.toybox.material-sbsar";
const enc = (text) => new TextEncoder().encode(text);
const dist = (file) => import(pathToFileURL(path.join(pluginDir(ID), "dist", file)).href);

const input = (identifier, type, guiWidget, value, extra = {}) => ({
  id: identifier.length * 1000 + type.length, identifier, label: identifier.replace(/_/g, " "), type, guiWidget, guiGroup: "", guiDescription: "",
  value, defaultValue: value, minValue: 0, maxValue: 1, sliderStep: 0, visibleIf: true, labelTrue: "True", labelFalse: "False", enumValues: [], ...extra,
});
const output = (identifier, use = identifier) => ({ identifier, label: identifier, channelUse: [use], defaultChannelUse: use });
const preset = (label, values) => ({ label, embedded: true, value: `<sbspreset>\n${Object.entries(values).map(([k, v]) => `<presetinput identifier="${k}" uid="1" type="1" value="${v}" />`).join("\n")}\n</sbspreset>` });

const PNG_END = Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0, 0, 0, 0]);

/** A graph the way the engine reports it. `plain` has no switch and no colour. */
function tiles(index, label, { plain = false } = {}) {
  return {
    index, label,
    inputs: [
      input("$outputsize", "integer2", "NoWidget", [9, 9]), input("normal_format", "integer", "Combobox", 1),
      input("joint_height", "float", "Slider", 0.4, { guiGroup: "Joints" }),
      input("glass_selection", "integer", "Combobox", 1, { maxValue: 0, enumValues: [{ first: 0, second: "Square" }, { first: 1, second: "Round" }] }),
      input("count", "integer", "Slider", 12, { minValue: 1, maxValue: 24 }),
      ...(plain ? [] : [input("glow", "integer", "Togglebutton", 0), input("tint", "float3", "Color", [0.5, 0.5, 0.5])]),
    ],
    outputs: [output("basecolor", "baseColor"), output("normal"), output("roughness")],
    presets: [preset("Green", { joint_height: "0.8" }), preset("Blue", { joint_height: "0.1", count: "3" })],
  };
}

/** The engine as the plugin sees it: JSON messages in, files written on render. */
function fakeEngine(state) {
  return {
    isOpen: async () => state.open,
    requestJson: async (port, message) => {
      state.calls.push({ port, operation: message.operation, data: message.data });
      switch (message.operation) {
        case 10: return { code: 0, data: state.hello ?? {} };
        case 20: {
          const file = String(message.data.substance.file);
          const graphs = state.graphs(path.basename(file));
          return { code: 0, data: { uuid: "U1", graphs } };
        }
        case 24: return { code: 0, data: { graph: { inputs: [] } } };
        case 25: {
          if (state.failRender) return { code: 9, message: "the engine crashed while rendering" };
          const dir = message.data.render.path;
          mkdirSync(dir, { recursive: true });
          for (const { identifier } of message.data.render.outputs) writeFileSync(path.join(dir, `tiles_Tiles_${identifier}.png`), PNG_END);
          return { code: 0 };
        }
        default: return { code: 1, message: `unexpected operation ${message.operation}` };
      }
    },
  };
}

const ops = (state, operation) => state.calls.filter((call) => call.operation === operation);
const updates = (state) => ops(state, 24).map((call) => [call.data.input.identifier, call.data.input.value]);
const renders = (scratch) => (existsSync(scratch) ? readdirSync(scratch).filter((name) => name.startsWith("render-")) : []);

async function setup(files, over = {}) {
  const state = { calls: [], open: true, launched: [], graphs: (name) => [tiles(0, "Tiles", { plain: name.startsWith("plain") })], ...over.state };
  const src = writeTree(tempDir("sbsar-src"), files);
  const root = tempDir("library");
  const engineFile = writeTree(tempDir("engine"), { "substance_remote_engine.exe": "x" });
  const services = defaultServices({
    local: fakeEngine(state),
    tools: { launch: async (setting, args) => { state.launched.push({ setting, args }); state.open = true; }, run: async () => ({ code: 0, stdout: "", stderr: "" }), hostProcessId: async () => 77 },
    settings: { get: async () => ({ size: "10", engine: path.join(engineFile, "substance_remote_engine.exe"), renderer: "auto", ...over.settings }), set: async () => {} },
  });
  const scratch = await services.scratchDir();
  const host = await loadPlugins(["com.toybox.material-core", ID], { libraryRoot: root, services, store: true });
  return { state, src, root, host, scratch, engineFile };
}

const archives = { "tiles.sbsar": enc("archive one"), "notes.txt": "x" };
const run = (host, src, options, name = "tiles.sbsar") => host.runImporter(ID, "sbsar", [path.join(src, name)], options);

// --- importing -----------------------------------------------------------------

test("tbx import sbsar renders an archive with its defaults and stages maps and archive", async () => {
  const { state, src, root, host, scratch } = await setup(archives);
  const result = await host.runImporter(ID, "sbsar", [src], { category: "ceramic" });

  assert.equal(host.committed.length, 1);
  const [entry] = host.committed;
  assert.equal(entry.kind, "material");
  assert.equal(entry.name, "tiles");
  assert.equal(entry.category, "ceramic");
  assert.equal(entry.format, "sbsar");
  assert.deepEqual(entry.maps, [
    { role: "albedo", file: "albedo.png" },
    { role: "normal", file: "normal.png" },
    { role: "roughness", file: "roughness.png", channel: "r" },
  ]);
  assert.deepEqual(entry.origin.files, ["tiles.sbsar"]);
  assert.deepEqual({ ...entry.origin.meta, archiveHash: undefined }, { sbsar: "tiles.sbsar", graph: 0, preset: null, inputs: {}, size: 10, archiveHash: undefined });
  assert.match(entry.origin.meta.archiveHash, /^[0-9a-f]+-[0-9a-f]{16}$/);
  assert.deepEqual(listTree(root).map((file) => file.split("/").slice(1).join("/").replace(/materials\/[^/]+\//, "materials/<key>/")), ["materials/<key>/albedo.png", "materials/<key>/normal.png", "materials/<key>/roughness.png", "materials/<key>/tiles.sbsar"]);
  assert.deepEqual(result.imported, [entry.id]);
  const load = ops(state, 20)[0];
  assert.equal(load.data.substance.file, path.join(src, "tiles.sbsar").replace(/\\/g, "/"));
  assert.deepEqual(updates(state), [["$outputsize", [10, 10]]]);
  // The render folder this run made is gone again.
  assert.deepEqual(renders(scratch), []);
});

test("tbx import sbsar --size, --preset and --all-presets choose what is rendered", async () => {
  const { state, src, host } = await setup({ "tiles.sbsar": enc("archive") });
  await run(host, src, { size: "512", preset: "Green" });
  // One chosen preset names no variant; the preset is recorded and its values were sent to the engine.
  assert.equal(host.committed[0].name, "tiles");
  assert.equal(host.committed[0].origin.meta.preset, "Green");
  assert.equal(host.committed[0].origin.meta.size, 9);
  assert.deepEqual(updates(state), [["joint_height", 0.8]]);
  host.committed.length = 0;
  await host.runImporter(ID, "sbsar", [src], { allPresets: true });
  assert.deepEqual(host.committed.map((entry) => entry.name).sort(), ["tiles - Blue", "tiles - Green"]);
});

test("tbx import sbsar says why nothing was imported", async () => {
  const { src, host } = await setup(archives);
  await assert.rejects(host.runImporter(ID, "sbsar", [src], { preset: "Missing" }), /tiles: has no preset named "Missing"[^]*it has: Green, Blue/);
  await assert.rejects(host.runImporter(ID, "sbsar", [path.join(src, "notes.txt")]), /No Substance archives|not a Substance archive/);
  await assert.rejects(host.runImporter(ID, "sbsar", [tempDir("empty")]), /No Substance archives \(\.sbsar\) found/);
  await assert.rejects(host.runImporter(ID, "sbsar", [src], { category: "a/b" }), /category/);
  assert.equal(host.committed.length, 0);
});

test("a failure says which archive and which step, and the others go on", async () => {
  const { state, src, host, scratch } = await setup({ "tiles.sbsar": enc("a"), "plain.sbsar": enc("b") });
  state.graphs = (name) => { if (name === "plain.sbsar") throw new Error("The substance file is not valid"); return [tiles(0, "Tiles")]; };
  const result = await host.runImporter(ID, "sbsar", [src]);
  assert.deepEqual(host.committed.map((entry) => entry.name), ["tiles"]);
  assert.equal(result.skipped.length, 1);
  assert.match(result.skipped[0], /^plain: loading in the engine: /);

  state.graphs = () => [tiles(0, "Tiles")];
  state.failRender = true;
  host.committed.length = 0;
  await assert.rejects(host.runImporter(ID, "sbsar", [src]), /Nothing could be rendered\. plain: rendering 1024 px: the engine crashed while rendering; tiles: rendering 1024 px: the engine crashed while rendering/);
  // Not even a failed render leaves its folder behind.
  assert.deepEqual(renders(scratch), []);
});

test("an engine that is unreachable is reported per archive", async () => {
  const src = writeTree(tempDir("sbsar-src"), { "tiles.sbsar": enc("archive") });
  const services = defaultServices({ local: { isOpen: async () => true, requestJson: async () => ({ code: 7, message: "engine said no" }) } });
  const host = await loadPlugins([ID], { libraryRoot: tempDir("library"), services });
  await assert.rejects(host.runImporter(ID, "sbsar", [src]), /Nothing could be rendered\. tiles: loading in the engine: engine said no/);
});

// --- parameters ----------------------------------------------------------------

test("--set takes numbers, switches, list entries, colours and vectors, and records what changed", async () => {
  const { state, src, host } = await setup(archives);
  await run(host, src, { size: "512", preset: "Blue", set: ["joint_height=0.9", "glass_selection=Square", "glow=true", "tint=#ff0000", "count=20"] });
  assert.deepEqual(updates(state), [["joint_height", 0.9], ["glass_selection", 0], ["count", 20], ["glow", 1], ["tint", [1, 0, 0]]]);
  const meta = host.committed[0].origin.meta;
  assert.equal(meta.preset, "Blue");
  // Recorded: what differs from the preset ("Blue" already gives count 3 and joint_height 0.1).
  assert.deepEqual(meta.inputs, { joint_height: 0.9, glass_selection: 0, count: 20, glow: 1, tint: [1, 0, 0] });
  host.committed.length = 0;
  await run(host, src, { size: "512", set: ["tint=0.1, 0.2,0.3"] });
  assert.deepEqual(host.committed[0].origin.meta.inputs, { tint: [0.1, 0.2, 0.3] });
});

test("--values reads a JSON object, and --set wins over it", async () => {
  const { state, src, host } = await setup(archives);
  const values = path.join(writeTree(tempDir("values"), { "v.json": JSON.stringify({ joint_height: 0.2, count: 5, tint: [0, 1, 0] }) }), "v.json");
  await run(host, src, { values, set: ["count=6"] });
  assert.deepEqual(updates(state), [["joint_height", 0.2], ["count", 6], ["tint", [0, 1, 0]], ["$outputsize", [10, 10]]]);
  const bad = path.join(writeTree(tempDir("values"), { "bad.json": "[1,2]", "worse.json": "{" }), "bad.json");
  await assert.rejects(run(host, src, { values: bad }), /must hold one JSON object/);
  await assert.rejects(run(host, src, { values: path.join(path.dirname(bad), "worse.json") }), /not valid JSON/);
  await assert.rejects(run(host, src, { values: path.join(path.dirname(bad), "none.json") }), /cannot read/);
});

test("a value that does not fit says what is allowed; an unknown name lists close matches", async () => {
  const { src, host } = await setup(archives);
  const fails = async (set, pattern) => assert.rejects(run(host, src, { set }), pattern, set.join(" "));
  await fails(["count=99"], /count: 99 is above the maximum \(allowed 1 to 24\)/);
  await fails(["count=0"], /count: 0 is below the minimum \(allowed 1 to 24\)/);
  await fails(["count=2.5"], /count: 2.5 is not a whole number/);
  await fails(["joint_height=high"], /joint_height: "high" is not a number/);
  await fails(["glass_selection=Hex"], /glass_selection: "Hex" is not one of 0 = Square, 1 = Round/);
  await fails(["glow=maybe"], /glow: "maybe" is not true or false/);
  await fails(["tint=0.1,0.2"], /tint: "0.1,0.2" must be 3 numbers separated by commas/);
  await fails(["tint=#12"], /tint: "#12" is not a colour like #rrggbb/);
  await fails(["joint_hight=0.3"], /has no parameter "joint_hight"; did you mean joint_height\?/);
  await fails(["wholly_other=1"], /has no parameter "wholly_other"; `tbx sbsar inputs <file>` lists its parameters/);
  await fails(["normal_format=0"], /set by Toybox itself/);
  await fails(["nonsense"], /not name=value/);
  assert.equal(host.committed.length, 0);
});

test("with several archives a missing parameter skips that archive only, unless --ignore-missing", async () => {
  const { src, host } = await setup({ "tiles.sbsar": enc("a"), "plain.sbsar": enc("b") });
  const result = await host.runImporter(ID, "sbsar", [src], { set: ["glow=true"] });
  assert.deepEqual(host.committed.map((entry) => entry.name), ["tiles"]);
  assert.equal(result.skipped.length, 1);
  assert.match(result.skipped[0], /^plain: has no parameter "glow"/);
  host.committed.length = 0;
  const lenient = await host.runImporter(ID, "sbsar", [src], { set: ["glow=true"], ignoreMissing: true });
  assert.deepEqual(host.committed.map((entry) => entry.name).sort(), ["plain", "tiles"]);
  assert.deepEqual(lenient.skipped, []);
  assert.ok(lenient.warnings.some((warning) => /plain: has no parameter "glow"/.test(warning)));
});

test("--graph picks one graph of an archive that holds several, by number or label", async () => {
  const { state, src, host } = await setup({ "tiles.sbsar": enc("a") }, { state: { graphs: () => [tiles(0, "Tiles"), tiles(1, "Bricks")] } });
  await run(host, src, {});
  assert.equal(host.committed[0].origin.meta.graph, 0);
  host.committed.length = 0;
  await run(host, src, { graph: "bricks" });
  assert.equal(host.committed[0].name, "tiles - Bricks");
  assert.deepEqual([host.committed[0].origin.meta.graph, host.committed[0].origin.meta.graphLabel], [1, "Bricks"]);
  assert.ok(ops(state, 24).some((call) => call.data.substance.graph_idx === 1));
  host.committed.length = 0;
  await run(host, src, { graph: "1" });
  assert.equal(host.committed[0].name, "tiles - Bricks");
  await assert.rejects(run(host, src, { graph: "7" }), /no graph 7 \(it has 0: Tiles, 1: Bricks\)/);
});

// --- names and replacing -------------------------------------------------------

test("--name names the material instead of the archive, so variants of one archive sit side by side", async () => {
  const { src, host } = await setup(archives);
  const first = await run(host, src, { name: "tiles warm", set: ["count=7"], size: "512" });
  const second = await run(host, src, { name: "tiles cool", set: ["count=3"], size: "512" });
  const plain = await run(host, src, { size: "512" });
  assert.deepEqual(host.committed.map((entry) => entry.name), ["tiles warm", "tiles cool", "tiles"]);
  assert.equal(new Set(host.committed.map((entry) => entry.id)).size, 3, "three materials, three ids");
  assert.equal(host.library.size, 3, "none replaced another");
  assert.ok(host.committed.every((entry) => entry.origin.meta.sbsar === "tiles.sbsar"), "all rendered from the same archive");
  assert.equal(first.replaced, undefined);
  assert.equal(second.replaced, undefined);
  assert.equal(plain.replaced, undefined);
});

test("--name with --all-presets is the base of <name> - <preset>; a name is cleaned like any other", async () => {
  const { src, host } = await setup(archives);
  await run(host, src, { name: "kitchen", allPresets: true });
  assert.deepEqual(host.committed.map((entry) => entry.name).sort(), ["kitchen - Blue", "kitchen - Green"]);
  host.committed.length = 0;
  await run(host, src, { name: "a/b: c" });
  assert.equal(host.committed[0].name, "a_b_ c");
  await assert.rejects(run(host, src, { name: "  " }), /--name needs a name/);
});

test("--name with several archives is refused before anything renders", async () => {
  const { state, src, host } = await setup({ "tiles.sbsar": enc("a"), "plain.sbsar": enc("b") });
  await assert.rejects(host.runImporter(ID, "sbsar", [src], { name: "both" }), /--name gives one material its name, but 2 archives are selected/);
  assert.equal(state.calls.length, 0, "the engine was never asked");
  assert.equal(host.committed.length, 0);
});

test("an import that replaces a material says which and what it was", async () => {
  const { src, host } = await setup(archives);
  const first = await run(host, src, { preset: "Green", set: ["count=7"], size: "512" });
  assert.equal(first.replaced, undefined, "nothing was replaced the first time");
  const id = host.committed[0].id;
  const again = await run(host, src, { set: ["count=2"], size: "1024" });
  assert.deepEqual(again.replaced, [{ id, name: "tiles", was: "preset Green, count=7, 512 px" }]);
  // Rendering the same as what is there replaces it too (without --skip-existing), and the default one says "defaults".
  const third = await run(host, src, { size: "1024" });
  assert.deepEqual(third.replaced.map((item) => item.was), ["defaults, count=2, 1024 px"]);
  // A material of the same name replaced by --name is the same thing.
  const named = await run(host, src, { name: "tiles" });
  assert.equal(named.replaced.length, 1);
  assert.equal(host.library.size, 1);
});

// --- skipping what is already there ---------------------------------------------

test("--skip-existing leaves alone a material rendered from the same archive, preset, parameters and size", async () => {
  const { state, src, host } = await setup(archives);
  await run(host, src, { skipExisting: true, size: "512" });
  assert.equal(host.committed.length, 1);
  const first = state.calls.length;

  // Same archive, defaults, same size: skipped without even starting the engine.
  host.committed.length = 0;
  const again = await run(host, src, { skipExisting: true, size: "512" });
  assert.equal(host.committed.length, 0);
  assert.deepEqual(again.imported, []);
  assert.deepEqual(again.skipped ?? [], []);
  assert.equal(state.calls.length, first, "no engine call was made");
  assert.ok(host.logs.some((line) => /Left 1 item alone, already in the library: tiles/.test(line)));

  // With a preset and parameters the engine is asked about the archive, but nothing is rendered.
  await run(host, src, { skipExisting: true, size: "512", preset: "Green", set: ["count=7"] });
  assert.equal(host.committed.length, 1);
  host.committed.length = 0;
  const renderCalls = ops(state, 25).length;
  await run(host, src, { skipExisting: true, size: "512", preset: "Green", set: ["count=7"] });
  assert.equal(host.committed.length, 0);
  assert.equal(ops(state, 25).length, renderCalls);

  // Anything that differs renders again: parameter, size, archive content.
  await run(host, src, { skipExisting: true, size: "512", preset: "Green", set: ["count=8"] });
  assert.equal(host.committed.length, 1);
  host.committed.length = 0;
  await run(host, src, { skipExisting: true, size: "1024", preset: "Green", set: ["count=8"] });
  assert.equal(host.committed.length, 1);
  host.committed.length = 0;
  writeFileSync(path.join(src, "tiles.sbsar"), "a different archive");
  await run(host, src, { skipExisting: true, size: "1024", preset: "Green", set: ["count=8"] });
  assert.equal(host.committed.length, 1);
  // Without the flag an existing material is simply replaced.
  host.committed.length = 0;
  await run(host, src, { size: "1024", preset: "Green", set: ["count=8"] });
  assert.equal(host.committed.length, 1);
});

// --- rendering stored materials again ------------------------------------------

async function imported(over = {}) {
  const made = await setup(archives, over);
  await run(made.host, made.src, { size: "512", preset: "Green" });
  const [entry] = made.host.committed;
  made.host.committed.length = 0;
  made.state.calls.length = 0;
  return { ...made, id: entry.id };
}
const render = (made, options = {}, ids) => made.host.runCommand(ID, "sbsar render", {}, options, ids ?? [made.id]);

test("tbx sbsar render updates a material in place: same id, tags kept, new values recorded", async () => {
  const made = await imported();
  made.host.library.get(made.id).user.tags = ["floor"];
  const before = listTree(made.root);
  const result = await render(made, { set: ["count=9"], size: "1024" });
  assert.equal(made.host.committed.length, 1);
  const [entry] = made.host.committed;
  assert.equal(entry.id, made.id);
  assert.equal(entry.name, "tiles");
  assert.deepEqual(entry.origin.meta, { ...entry.origin.meta, preset: "Green", inputs: { count: 9 }, size: 10 });
  // Started from the material's own preset; only the change was sent besides the preset's values.
  assert.deepEqual(updates(made.state), [["joint_height", 0.8], ["count", 9], ["$outputsize", [10, 10]]]);
  assert.deepEqual(made.host.library.get(made.id).user.tags, ["floor"]);
  assert.equal(made.host.library.get(made.id).origin.meta.size, 10);
  assert.equal(listTree(made.root).length, before.length, "the old folder is gone, the new one is in");
  assert.deepEqual(result.updated.map((item) => item.id), [made.id]);
  assert.deepEqual(result.skipped, []);
  assert.deepEqual(made.host.printed.at(-1), made.id);
  assert.deepEqual(renders(made.scratch), []);
});

test("render --reset, --preset and --save-as", async () => {
  const made = await imported();
  await render(made, { reset: true });
  assert.deepEqual([made.host.committed[0].origin.meta.preset, made.host.committed[0].origin.meta.inputs, made.host.committed[0].origin.meta.size], [null, {}, 9]);
  made.host.committed.length = 0;
  await render(made, { preset: "Blue", set: ["count=4"] });
  assert.deepEqual([made.host.committed[0].origin.meta.preset, made.host.committed[0].origin.meta.inputs], ["Blue", { count: 4 }]);
  await assert.rejects(render(made, { reset: true, preset: "Blue" }), /cannot be combined/);

  made.host.committed.length = 0;
  const result = await render(made, { saveAs: "tiles green", preset: "Green" });
  const [created] = made.host.committed;
  assert.notEqual(created.id, made.id);
  assert.equal(created.name, "tiles green");
  assert.deepEqual(result.created.map((item) => item.id), [created.id]);
  assert.equal(made.host.library.get(made.id).name, "tiles", "the original is kept");
  assert.equal(made.host.library.size, 2);
  assert.ok(made.host.library.get(created.id).origin.files[0].endsWith("tiles.sbsar"), "the new material has its own copy of the archive");
});

test("render: --save-as needs one item, a bad value skips the item, other items are skipped with a reason", async () => {
  const made = await imported();
  made.host.library.set("other", { id: "other", name: "Plain", kind: "material", category: "c", origin: { plugin: "com.toybox.material-core" }, user: { favorite: false, tags: [] } });
  await assert.rejects(render(made, { saveAs: "x" }, [made.id, "other"]), /one new material, but 2 items are selected/);
  const bad = await render(made, { set: ["count=99"] }, [made.id, "other"]);
  assert.deepEqual(bad.updated, []);
  assert.equal(bad.skipped.length, 2);
  assert.match(bad.skipped[0], /tiles .*count: 99 is above the maximum/);
  assert.match(bad.skipped[1], /Plain .*not rendered from a Substance archive/);
  assert.equal(made.host.committed.length, 0);
  assert.equal(made.host.printed.at(-1), "Nothing was rendered.");
  // One good item is rendered even when the other is skipped.
  const mixed = await render(made, {}, [made.id, "other"]);
  assert.equal(mixed.updated.length, 1);
  assert.equal(mixed.skipped.length, 1);
  await assert.rejects(made.host.runCommand(ID, "sbsar render", {}, {}, []), /No item is selected/);
});

test("render --dry-run says what each would be rendered with and renders nothing", async () => {
  const made = await imported();
  const result = await render(made, { dryRun: true, set: ["count=9"], preset: "Blue" });
  assert.equal(made.host.committed.length, 0);
  assert.equal(ops(made.state, 25).length, 0);
  assert.equal(result.wouldRender.length, 1);
  assert.equal(result.wouldRender[0].preset, "Blue");
  assert.match(made.host.printed.at(-1), /tiles: would update in place from tiles\.sbsar, preset Blue, .*count=9.*, 512 px/);
  const copy = await render(made, { dryRun: true, saveAs: "copy" });
  assert.match(made.host.printed.at(-1), /would save as "copy"/);
  assert.equal(copy.wouldRender[0].mode, 'new "copy"');
});

test("render skips a material whose archive is not stored, and --ignore-missing drops unknown parameters", async () => {
  const made = await imported();
  const asset = made.host.library.get(made.id);
  const files = asset.origin.files;
  asset.origin.files = [];
  const none = await render(made, {});
  assert.match(none.skipped[0], /tiles\.sbsar\) is not stored with it/);
  asset.origin.files = files;
  const missing = await render(made, { set: ["nothing_like_it=1"], ignoreMissing: true });
  assert.equal(missing.updated.length, 1);
  assert.ok(made.host.warnings.some((warning) => /has no parameter "nothing_like_it"/.test(warning)));
});

test("tbx sbsar show tells how materials were made", async () => {
  const made = await imported();
  made.host.library.set("other", { id: "other", name: "Plain", kind: "material", origin: { plugin: "x" }, user: {} });
  await render(made, { set: ["count=9"] });
  made.host.printed.length = 0;
  const shown = await made.host.runCommand(ID, "sbsar show", {}, {}, [made.id, "other"]);
  assert.equal(shown.items.length, 2);
  assert.deepEqual({ ...shown.items[0], archiveHash: undefined, id: undefined }, {
    id: undefined, name: "tiles", library: "lib-1", substance: true, archive: "tiles.sbsar", graph: 0, preset: "Green", inputs: { count: 9 }, size: 9, pixels: 512, archiveStored: true, archiveHash: undefined,
  });
  assert.equal(shown.items[1].substance, false);
  assert.match(made.host.printed[0], /archive: {4}tiles\.sbsar[^]*preset: {5}Green[^]*parameters: count=9[^]*size: {7}512 px/);
  assert.match(made.host.printed[1], /not a Substance material/);
});

// --- looking inside, the engine --------------------------------------------------

test("tbx sbsar inputs lists graphs, outputs, presets and the parameters that can be changed", async () => {
  const { src, host } = await setup(archives);
  const result = await host.runCommand(ID, "sbsar inputs", { file: path.join(src, "tiles.sbsar") });
  assert.equal(result.graphs.length, 1);
  const [graph] = result.graphs;
  assert.equal(graph.label, "Tiles");
  assert.deepEqual(graph.presets, ["Green", "Blue"]);
  assert.deepEqual(graph.outputs.map((item) => item.identifier), ["basecolor", "normal", "roughness"]);
  // The texture size and the normal convention are the plugin's to set.
  assert.deepEqual(graph.inputs.map((item) => item.identifier), ["joint_height", "glass_selection", "count", "glow", "tint"]);
  assert.deepEqual(graph.inputs[0].range, { min: 0, max: 1, step: 0.01 });
  assert.deepEqual(graph.inputs[2].range, { min: 1, max: 24, step: 1 });
  assert.equal(graph.inputs[1].choices.length, 2);
  assert.equal(host.printed.length, 1);
  assert.match(host.printed[0], /Tiles \(graph 0\)[^]*presets: Green, Blue[^]*joint_height {2}float {2}default 0\.4; range 0\.\.1; group Joints[^]*glow {2}integer {2}default 0; true\/false[^]*tint {2}float3 {2}default 0\.5,0\.5,0\.5; colour, also as #808080/);
  await assert.rejects(host.runCommand(ID, "sbsar inputs", { file: path.join(src, "readme.txt") }), /not a Substance archive/);
});

test("sbsar inputs --preset shows the preset's values, --graph picks a graph", async () => {
  const { src, host } = await setup(archives, { state: { graphs: () => [tiles(0, "Tiles"), tiles(1, "Bricks")] } });
  const file = path.join(src, "tiles.sbsar");
  const result = await host.runCommand(ID, "sbsar inputs", { file }, { preset: "Blue" });
  assert.deepEqual(result.graphs[0].inputs.filter((item) => item.value !== undefined).map((item) => [item.identifier, item.value]).slice(0, 3), [["joint_height", 0.1], ["glass_selection", 1], ["count", 3]]);
  assert.match(host.printed[0], /count {2}integer {2}value 3; default 12/);
  host.printed.length = 0;
  const one = await host.runCommand(ID, "sbsar inputs", { file }, { graph: "Bricks" });
  assert.deepEqual(one.graphs.map((item) => item.label), ["Bricks"]);
  await assert.rejects(host.runCommand(ID, "sbsar inputs", { file }, { graph: "nope" }), /no graph nope \(it has 0: Tiles, 1: Bricks\)/);
  await assert.rejects(host.runCommand(ID, "sbsar inputs", { file }, { preset: "Red" }), /no preset named "Red"/);
});

test("tbx sbsar engine checks where the engine is and, with --start, that it answers", async () => {
  const { state, host, engineFile } = await setup(archives, { state: { open: false } });
  const plain = await host.runCommand(ID, "sbsar engine");
  assert.equal(plain.exists, true);
  assert.equal(plain.started, false);
  assert.equal(state.launched.length, 0, "without --start nothing is started");
  assert.match(host.printed.join("\n"), /Substance engine: .*substance_remote_engine\.exe\nRender with: {6}auto[^]*not tried/);

  state.hello = { version: "9.9.1" };
  host.printed.length = 0;
  const started = await host.runCommand(ID, "sbsar engine", {}, { start: true, renderer: "CPU" });
  assert.equal(started.started, true);
  assert.equal(state.launched.length, 1);
  assert.ok(state.launched[0].args.includes("-e=CPU"), "the renderer given for this run is used");
  assert.deepEqual(started.reply, { version: "9.9.1" });
  assert.match(host.printed.join("\n"), /answered after .* s; it said {"version":"9\.9\.1"}/);

  const missing = await setup(archives, { settings: { engine: path.join(engineFile, "gone", "engine.exe") } });
  await assert.rejects(missing.host.runCommand(ID, "sbsar engine"), /not found at .*engine\.exe\.\nhint: .*tbx plugin settings com\.toybox\.material-sbsar engine <path/);
});

test("--renderer overrides the setting for the run that starts the engine", async () => {
  const { state, src, host } = await setup(archives, { state: { open: false }, settings: { renderer: "VK" } });
  await run(host, src, { renderer: "CPU" });
  assert.ok(state.launched[0].args.includes("-e=CPU"));
  state.open = false;
  host.committed.length = 0;
  await run(host, src, {});
  assert.ok(state.launched[1].args.includes("-e=VK"), "the setting applies when no override is given");
});

// --- scratch ---------------------------------------------------------------------

test("a run removes the render folders it made and leaves other processes' folders alone", async () => {
  const { src, host, scratch } = await setup(archives);
  mkdirSync(path.join(scratch, "render-foreign-1"), { recursive: true });
  writeFileSync(path.join(scratch, "render-foreign-1", "map.png"), "in use by the app");
  await run(host, src, {});
  await run(host, src, { preset: "Green" });
  assert.deepEqual(renders(scratch), ["render-foreign-1"]);
  assert.ok(existsSync(path.join(scratch, "render-foreign-1", "map.png")));
});

// --- units -----------------------------------------------------------------------

test("parameter checks: close matches, colours, lists by label", async () => {
  const { closeMatches, coerceInput, splitSetting, parseValuesFile } = await dist("lib/params.js");
  assert.deepEqual(closeMatches("joint_hight", ["joint_height", "count", "tint"]), ["joint_height"]);
  assert.deepEqual(closeMatches("colour", ["color", "colour_a", "height"]).sort(), ["color", "colour_a"]);
  assert.deepEqual(closeMatches("zzz", ["joint_height", "count"]), []);
  assert.deepEqual(splitSetting("a=b=c"), { name: "a", value: "b=c" });
  assert.equal(splitSetting("=x"), undefined);
  const colour = input("c", "float4", "Color", [0, 0, 0, 1]);
  assert.deepEqual(coerceInput(colour, "#ff000080"), { value: [1, 0, 0, 128 / 255] });
  assert.deepEqual(coerceInput(colour, [0.1, 0.2, 0.3, 0.4]), { value: [0.1, 0.2, 0.3, 0.4] });
  assert.ok("problem" in coerceInput(input("c", "float2", "Color", [0, 0]), "#ff0000"));
  const list = input("l", "integer", "Combobox", 1, { enumValues: [{ value: 0, label: "Square" }, { value: 1, label: "Round" }] });
  assert.deepEqual(coerceInput(list, "round"), { value: 1 });
  assert.deepEqual(coerceInput(list, 0), { value: 0 });
  assert.ok("problem" in coerceInput(input("image", "image", "Image", ""), "x"));
  assert.ok("problem" in coerceInput(input("$randomseed", "integer", "NoWidget", 0), "1.5"));
  assert.deepEqual(coerceInput(input("$randomseed", "integer", "NoWidget", 0, { maxValue: 1 }), "7"), { value: 7 }, "the random seed has no range");
  assert.deepEqual(parseValuesFile('{"a":1,"b":"x","c":[1,2],"d":true}'), { a: 1, b: "x", c: [1, 2], d: true });
  assert.throws(() => parseValuesFile('{"a":{"b":1}}'), /must be a number/);
});

test("the archive module is the one place that stages, records and locates the archive", async () => {
  const { stageArchive, recordArchive, locateArchive, archiveFile, fingerprint, absolutePath } = await dist("lib/archive.js");
  const src = writeTree(tempDir("a"), { "deep/x.sbsar": "data" });
  const root = tempDir("lib");
  const fs = (await import("./support/cli-host.mjs")).createFs();
  const name = await stageArchive(fs, root, "_import_p", "Mat", { root: src, path: "deep/x.sbsar" });
  assert.equal(name, "x.sbsar");
  assert.deepEqual(listTree(root), ["library/_import_p/materials/Mat/x.sbsar"]);
  assert.deepEqual(recordArchive("x.sbsar", "ab-cd"), { files: ["x.sbsar"], meta: { sbsar: "x.sbsar", archiveHash: "ab-cd" } });
  const asset = { origin: { plugin: ID, files: ["library/materials/k/albedo.png", "library/materials/k/x.sbsar"] } };
  assert.equal(archiveFile(asset, { sbsar: "x.sbsar" }), "library/materials/k/x.sbsar");
  assert.deepEqual(locateArchive(asset, { sbsar: "x.sbsar" }, root), { root, path: "library/materials/k/x.sbsar" });
  assert.equal(locateArchive(asset, { sbsar: "y.sbsar" }, root), undefined);
  assert.equal(absolutePath({ root: "C:\\lib\\", path: "a\\b.sbsar" }), "C:/lib/a/b.sbsar");
  const a = fingerprint(new Uint8Array([1, 2, 3, 4, 5, 6, 7]));
  assert.equal(a, fingerprint(new Uint8Array([1, 2, 3, 4, 5, 6, 7])));
  assert.notEqual(a, fingerprint(new Uint8Array([1, 2, 3, 4, 5, 6, 8])));
  assert.notEqual(fingerprint(new Uint8Array([0, 0, 0, 0])), fingerprint(new Uint8Array([0, 0, 0, 0, 0])));
  // An unaligned view hashes like an aligned copy.
  const big = new Uint8Array([9, 1, 2, 3, 4, 5, 6, 7, 8]);
  assert.equal(fingerprint(big.subarray(1)), fingerprint(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8])));
});

// --- the real engine (opt in) ----------------------------------------------------

test("the real Substance engine renders an archive (set TOYBOX_REAL_SBSAR=<folder with .sbsar files> to run)", { skip: !process.env.TOYBOX_REAL_SBSAR }, async () => {
  const folder = process.env.TOYBOX_REAL_SBSAR;
  const root = tempDir("real-library");
  const scratch = tempDir("real-scratch");
  const { createFs } = await import("./support/cli-host.mjs");
  const { spawn } = await import("node:child_process");
  const real = await import("node:net");
  const sockets = (port, message, timeoutMs = 30000) => new Promise((resolve, reject) => {
    const socket = real.connect(port, "127.0.0.1");
    let data = "";
    socket.setTimeout(timeoutMs, () => { socket.destroy(); reject(new Error("timeout")); });
    socket.on("connect", () => socket.write(JSON.stringify(message)));
    socket.on("data", (chunk) => { data += chunk; try { const parsed = JSON.parse(data); socket.destroy(); resolve(parsed); } catch { /* wait for the rest */ } });
    socket.on("error", reject);
  });
  const children = [];
  const engine = path.join(process.env.APPDATA ?? "", "Adobe", "Substance3DIntegrationTools", "substance_remote_engine.exe");
  const services = defaultServices({
    settings: { get: async () => ({ size: "9", engine, renderer: "auto" }), set: async () => {} },
    tools: { launch: async (_setting, args) => { children.push(spawn(engine, args, { cwd: path.dirname(engine), stdio: "ignore" })); }, run: async () => ({ code: 0, stdout: "", stderr: "" }), hostProcessId: async () => process.pid },
    local: {
      isOpen: (port) => new Promise((resolve) => { const s = real.connect(port, "127.0.0.1"); s.on("connect", () => { s.destroy(); resolve(true); }); s.on("error", () => resolve(false)); }),
      requestJson: sockets,
    },
    scratchDir: async () => scratch,
  });
  try {
    const host = await loadPlugins(["com.toybox.material-core", ID], { libraryRoot: root, services, store: true });
    const result = await host.runImporter(ID, "sbsar", [folder], { size: "512", skipExisting: true });
    assert.ok(result.imported.length >= 1);
    assert.ok(host.committed.every((entry) => entry.maps.some((map) => map.role === "albedo")));
    assert.deepEqual(renders(scratch), []);
    void createFs;
  } finally {
    for (const child of children) child.kill();
  }
});
