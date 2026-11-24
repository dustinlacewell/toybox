// `tbx import sbsar` and `tbx sbsar ...` (substances and their materials) against a fake Substance engine: the
// plugin's real command-line module and engine client, a fake program behind
// `services.local`, real temp folders behind `fs`, and a pretend library that
// moves staged folders into place and replaces items of the same id as the host
// does. Run `tsc -p plugins/com.toybox.material-sbsar/tsconfig.json` first.

import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
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
const materials = (host) => host.committed.filter((entry) => entry.kind === "material");
const substances = (host) => host.committed.filter((entry) => entry.kind === "plugin");
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const substanceOf = (bytes) => `uid://substance_${sha256(bytes).slice(0, 32)}`;
const TILES = substanceOf(enc("archive one"));

// --- importing -----------------------------------------------------------------

test("tbx import sbsar adds the archive as a substance and renders its default material", async () => {
  const { state, src, root, host, scratch } = await setup(archives);
  const result = await host.runImporter(ID, "sbsar", [src], { category: "ceramic" });

  // The substance comes first, then the material made from it.
  assert.deepEqual(host.committed.map((entry) => entry.kind), ["plugin", "material"]);
  const [substance, entry] = host.committed;
  assert.deepEqual({ ...substance, pack: undefined }, {
    kind: "plugin", type: "substance", id: TILES, pack: undefined, category: "ceramic", name: "tiles", format: "sbsar",
    files: ["tiles.sbsar"],
    meta: { sbsar: "tiles.sbsar", sha256: sha256(enc("archive one")), bytes: 11,
      graphs: [{ index: 0, label: "Tiles", presets: ["Green", "Blue"], parameters: 5, outputs: ["baseColor", "normal", "roughness"] }] },
  });
  assert.match(substance.pack, /^_import_[0-9a-f]+$/);
  assert.equal(entry.kind, "material");
  assert.equal(entry.name, "tiles");
  assert.equal(entry.category, "ceramic");
  assert.equal(entry.format, "sbsar");
  assert.deepEqual(entry.maps, [
    { role: "albedo", file: "albedo.png" },
    { role: "normal", file: "normal.png" },
    { role: "roughness", file: "roughness.png", channel: "r" },
  ]);
  // The archive belongs to the substance: the material names it and keeps no file of its own.
  assert.deepEqual(entry.origin, { derivedFrom: TILES, meta: { sbsar: "tiles.sbsar", graph: 0, preset: null, inputs: {}, size: 10 } });
  assert.deepEqual(listTree(root).map((file) => file.split("/").slice(1).join("/").replace(/^(materials|substance)\/[^/]+\//, "$1/<key>/")),
    ["materials/<key>/albedo.png", "materials/<key>/normal.png", "materials/<key>/roughness.png", "substance/<key>/tiles.sbsar"]);
  // Everything committed is imported, as tbx lists it (the substance too); a new substance is also said on a line of its own.
  assert.deepEqual(result.imported, [substance.id, entry.id]);
  assert.ok(host.logs.some((line) => /Added 1 substance \(archives new to this library\): tiles/.test(line)));
  assert.equal(host.library.get(TILES).type, "substance");
  assert.deepEqual((await host.host.getDerivedAssets(TILES)).map((item) => item.id), [entry.id]);
  const load = ops(state, 20)[0];
  assert.equal(load.data.substance.file, path.join(src, "tiles.sbsar").replace(/\\/g, "/"));
  assert.deepEqual(updates(state), [["$outputsize", [10, 10]]]);
  // The render folder this run made is gone again.
  assert.deepEqual(renders(scratch), []);
});

test("a substance gets preview.png from the first rendering: copied at 512 px, scaled down by the host above", async () => {
  const small = await setup(archives);
  await run(small.host, small.src, { size: "512" });
  assert.deepEqual(substances(small.host)[0].files, ["tiles.sbsar", "preview.png"]);
  assert.ok(listTree(small.root).some((file) => /^library\/substance\/[^/]+\/preview\.png$/.test(file)));

  const large = await setup(archives);
  const asked = [];
  large.host.fs.readImageThumb = async (root, file, max) => { asked.push([path.basename(file), max]); return enc("scaled"); };
  await run(large.host, large.src, { size: "2048" });
  assert.deepEqual(substances(large.host)[0].files, ["tiles.sbsar", "preview.png"]);
  // Scaled from the material's staged colour map, once the archive has given a material.
  assert.deepEqual(asked, [["albedo.png", 512]]);
  const stored = large.host.library.get(TILES).files.find((file) => file.endsWith("preview.png"));
  assert.equal(readFileSync(path.join(large.root, stored), "utf8"), "scaled");
  // A host that cannot scale images: no preview (the thumbnail is then a placeholder), nothing else changes.
  const none = await setup(archives);
  await run(none.host, none.src, { size: "1024" });
  assert.deepEqual(substances(none.host)[0].files, ["tiles.sbsar"]);
});

test("the same archive is one substance per library, wherever it is imported from", async () => {
  const { src, root, host } = await setup(archives);
  await run(host, src, {});
  const other = writeTree(tempDir("elsewhere"), { "copy of tiles.sbsar": enc("archive one"), "plain.sbsar": enc("another archive") });
  host.committed.length = 0;
  const result = await host.runImporter(ID, "sbsar", [other], {});
  // The copy reuses the substance (nothing staged for it); the new archive brings its own.
  assert.deepEqual(host.committed.map((entry) => `${entry.kind}:${entry.name}`), ["material:copy of tiles", "plugin:plain", "material:plain"]);
  assert.equal(materials(host)[0].origin.derivedFrom, TILES);
  assert.equal(materials(host)[1].origin.derivedFrom, substanceOf(enc("another archive")));
  assert.equal(result.imported.length, 3, "two materials and the new substance");
  assert.equal(listTree(root).filter((file) => file.endsWith(".sbsar")).length, 2, "one stored copy per archive content");
  assert.ok(!listTree(root).some((file) => /_import_/.test(file)), "nothing left in staging");
  // Two copies of one archive in one import are one substance too.
  const twice = await setup({ "a/x.sbsar": enc("same"), "b/y.sbsar": enc("same") });
  await twice.host.runImporter(ID, "sbsar", [path.join(twice.src, "a"), path.join(twice.src, "b")], {});
  assert.deepEqual(twice.host.committed.map((entry) => `${entry.kind}:${entry.name}`), ["plugin:x", "material:x", "material:y"]);
  // Two archives of one name in different folders of one import: different content is two substances, each with
  // its own staging folder and stored archive; the materials' names are made unique.
  const apart = await setup({ "a/tiles.sbsar": enc("first"), "b/tiles.sbsar": enc("second"), "c/tiles.sbsar": enc("first") });
  await apart.host.runImporter(ID, "sbsar", [apart.src], { size: "512" });
  assert.deepEqual(apart.host.committed.map((entry) => `${entry.kind}:${entry.name}`), ["plugin:tiles", "material:tiles", "plugin:tiles", "material:tiles 2", "material:tiles 3"]);
  const [one, two] = substances(apart.host);
  assert.deepEqual([one.id, two.id], [substanceOf(enc("first")), substanceOf(enc("second"))]);
  assert.notEqual(one.pack, two.pack);
  assert.deepEqual(materials(apart.host).map((entry) => entry.origin.derivedFrom), [one.id, two.id, one.id]);
  const stored = listTree(apart.root);
  assert.equal(stored.filter((file) => file.endsWith(".sbsar")).length, 2);
  assert.equal(stored.filter((file) => file.endsWith("preview.png")).length, 2);
  assert.deepEqual(stored.filter((file) => /^library\/substance\//.test(file)).map((file) => readFileSync(path.join(apart.root, file), "utf8")).filter((text) => text === "first" || text === "second").sort(), ["first", "second"]);
});

test("a new substance stages nothing of its own until its archive has given a material", async () => {
  // The archive cannot be copied (after its material rendered): no preview is left in staging, even on a host
  // that cannot remove files (the app), and its material is left out.
  const made = await setup(archives);
  const copy = made.host.fs.copyFile;
  made.host.fs.copyFile = async (from, file, root, target) => { if (target.endsWith(".sbsar")) throw new Error("disk full"); return copy(from, file, root, target); };
  delete made.host.fs.removeTree;
  await assert.rejects(run(made.host, made.src, { size: "512" }), /Nothing could be rendered\. tiles: copying the archive: disk full/);
  assert.ok(!listTree(made.root).some((file) => /\/substance\//.test(file)), "no preview or archive staged for the substance");
  assert.equal(made.host.committed.length, 0);
  // Where files can be removed (tbx), the left-out material's staged maps go too.
  const tidy = await setup(archives);
  const copy2 = tidy.host.fs.copyFile;
  tidy.host.fs.copyFile = async (from, file, root, target) => { if (target.endsWith(".sbsar")) throw new Error("disk full"); return copy2(from, file, root, target); };
  await assert.rejects(run(tidy.host, tidy.src, { size: "512" }), /copying the archive: disk full/);
  assert.deepEqual(listTree(tidy.root).filter((file) => !file.endsWith("/")), []);
});

test("tbx import sbsar --size, --preset and --all-presets choose what is rendered", async () => {
  const { state, src, host } = await setup({ "tiles.sbsar": enc("archive") });
  await run(host, src, { size: "512", preset: "Green" });
  // One chosen preset names no variant; the preset is recorded and its values were sent to the engine.
  assert.equal(materials(host)[0].name, "tiles");
  assert.equal(materials(host)[0].origin.meta.preset, "Green");
  assert.equal(materials(host)[0].origin.meta.size, 9);
  assert.deepEqual(updates(state), [["joint_height", 0.8]]);

  const every = await setup({ "tiles.sbsar": enc("archive") });
  await every.host.runImporter(ID, "sbsar", [every.src], { allPresets: true });
  // One substance, then a material per preset, all made from it.
  assert.deepEqual(every.host.committed.map((entry) => `${entry.kind}:${entry.name}`), ["plugin:tiles", "material:tiles - Green", "material:tiles - Blue"]);
  const id = substances(every.host)[0].id;
  assert.ok(materials(every.host).every((entry) => entry.origin.derivedFrom === id));
  assert.deepEqual((await every.host.host.getDerivedAssets(id)).map((item) => item.name).sort(), ["tiles - Blue", "tiles - Green"]);
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
  const { state, src, root, host, scratch } = await setup({ "tiles.sbsar": enc("a"), "plain.sbsar": enc("b") });
  state.graphs = (name) => { if (name === "plain.sbsar") throw new Error("The substance file is not valid"); return [tiles(0, "Tiles")]; };
  const result = await host.runImporter(ID, "sbsar", [src]);
  assert.deepEqual(materials(host).map((entry) => entry.name), ["tiles"]);
  // An archive nothing could be made from does not become a substance either.
  assert.deepEqual(substances(host).map((entry) => entry.name), ["tiles"]);
  assert.equal(result.skipped.length, 1);
  assert.match(result.skipped[0], /^plain: loading in the engine: /);

  state.graphs = () => [tiles(0, "Tiles")];
  state.failRender = true;
  host.committed.length = 0;
  await assert.rejects(host.runImporter(ID, "sbsar", [src]), /Nothing could be rendered\. plain: rendering 1024 px: the engine crashed while rendering; tiles: rendering 1024 px: the engine crashed while rendering/);
  // Not even a failed render leaves its folder behind, and nothing is left staged.
  assert.deepEqual(renders(scratch), []);
  assert.equal(host.committed.length, 0);
  assert.ok(!listTree(root).some((file) => file.includes("/_import_")), "no staged substance of a failed archive was committed");
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
  const meta = materials(host)[0].origin.meta;
  assert.equal(meta.preset, "Blue");
  // Recorded: what differs from the preset ("Blue" already gives count 3 and joint_height 0.1).
  assert.deepEqual(meta.inputs, { joint_height: 0.9, glass_selection: 0, count: 20, glow: 1, tint: [1, 0, 0] });
  host.committed.length = 0;
  await run(host, src, { size: "512", set: ["tint=0.1, 0.2,0.3"] });
  assert.deepEqual(materials(host)[0].origin.meta.inputs, { tint: [0.1, 0.2, 0.3] });
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
  assert.deepEqual(materials(host).map((entry) => entry.name), ["tiles"]);
  assert.equal(result.skipped.length, 1);
  assert.match(result.skipped[0], /^plain: has no parameter "glow"/);
  host.committed.length = 0;
  const lenient = await host.runImporter(ID, "sbsar", [src], { set: ["glow=true"], ignoreMissing: true });
  assert.deepEqual(materials(host).map((entry) => entry.name).sort(), ["plain", "tiles"]);
  assert.deepEqual(lenient.skipped, []);
  assert.ok(lenient.warnings.some((warning) => /plain: has no parameter "glow"/.test(warning)));
});

test("--graph picks one graph of an archive that holds several, by number or label", async () => {
  const { state, src, host } = await setup({ "tiles.sbsar": enc("a") }, { state: { graphs: () => [tiles(0, "Tiles"), tiles(1, "Bricks")] } });
  await run(host, src, {});
  assert.equal(materials(host)[0].origin.meta.graph, 0);
  // The substance records every graph of the archive.
  assert.deepEqual(substances(host)[0].meta.graphs.map((graph) => [graph.index, graph.label]), [[0, "Tiles"], [1, "Bricks"]]);
  host.committed.length = 0;
  await run(host, src, { graph: "bricks" });
  assert.equal(materials(host)[0].name, "tiles - Bricks");
  assert.deepEqual([materials(host)[0].origin.meta.graph, materials(host)[0].origin.meta.graphLabel], [1, "Bricks"]);
  assert.ok(ops(state, 24).some((call) => call.data.substance.graph_idx === 1));
  host.committed.length = 0;
  await run(host, src, { graph: "1" });
  assert.equal(materials(host)[0].name, "tiles - Bricks");
  await assert.rejects(run(host, src, { graph: "7" }), /no graph 7 \(it has 0: Tiles, 1: Bricks\)/);
});

// --- names and replacing -------------------------------------------------------

test("--name names the material instead of the archive, so variants of one archive sit side by side", async () => {
  const { src, host } = await setup(archives);
  const first = await run(host, src, { name: "tiles warm", set: ["count=7"], size: "512" });
  const second = await run(host, src, { name: "tiles cool", set: ["count=3"], size: "512" });
  const plain = await run(host, src, { size: "512" });
  assert.deepEqual(materials(host).map((entry) => entry.name), ["tiles warm", "tiles cool", "tiles"]);
  assert.equal(new Set(materials(host).map((entry) => entry.id)).size, 3, "three materials, three ids");
  // The substance came with the first import and took its name.
  assert.deepEqual(substances(host).map((entry) => entry.name), ["tiles warm"]);
  assert.equal(host.library.size, 4, "none replaced another: three materials and their substance");
  assert.ok(materials(host).every((entry) => entry.origin.meta.sbsar === "tiles.sbsar" && entry.origin.derivedFrom === TILES), "all rendered from the same substance");
  assert.equal(first.replaced, undefined);
  assert.equal(second.replaced, undefined);
  assert.equal(plain.replaced, undefined);
});

test("--name with --all-presets is the base of <name> - <preset>; a name is cleaned like any other", async () => {
  const { src, host } = await setup(archives);
  await run(host, src, { name: "kitchen", allPresets: true });
  assert.deepEqual(materials(host).map((entry) => entry.name).sort(), ["kitchen - Blue", "kitchen - Green"]);
  host.committed.length = 0;
  await run(host, src, { name: "a/b: c" });
  assert.equal(materials(host)[0].name, "a_b_ c");
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
  const id = materials(host)[0].id;
  const again = await run(host, src, { set: ["count=2"], size: "1024" });
  assert.deepEqual(again.replaced, [{ id, name: "tiles", was: "preset Green, count=7, 512 px" }]);
  assert.deepEqual(again.imported, [id], "only the material: its substance is in the library already");
  // Rendering the same as what is there replaces it too (without --skip-existing), and the default one says "defaults".
  const third = await run(host, src, { size: "1024" });
  assert.deepEqual(third.replaced.map((item) => item.was), ["defaults, count=2, 1024 px"]);
  // A material of the same name replaced by --name is the same thing.
  const named = await run(host, src, { name: "tiles" });
  assert.equal(named.replaced.length, 1);
  assert.equal(host.library.size, 2);
  assert.equal(substances(host).length, 1, "the substance was staged once");
});

// --- skipping what is already there ---------------------------------------------

test("--skip-existing leaves alone a material made from the same substance, preset, parameters and size", async () => {
  const { state, src, host } = await setup(archives);
  await run(host, src, { skipExisting: true, size: "512" });
  assert.equal(materials(host).length, 1);
  const first = state.calls.length;

  // Same archive, defaults, same size: skipped without even starting the engine (a hash and two lookups).
  host.committed.length = 0;
  const again = await run(host, src, { skipExisting: true, size: "512" });
  assert.equal(host.committed.length, 0);
  assert.deepEqual(again.imported, []);
  assert.deepEqual(again.skipped ?? [], []);
  assert.equal(state.calls.length, first, "no engine call was made");
  assert.ok(host.logs.some((line) => /Left 1 item alone, already in the library: tiles/.test(line)));

  // With a preset and parameters the engine is asked about the archive, but nothing is rendered.
  await run(host, src, { skipExisting: true, size: "512", preset: "Green", set: ["count=7"] });
  assert.equal(materials(host).length, 1);
  host.committed.length = 0;
  const renderCalls = ops(state, 25).length;
  await run(host, src, { skipExisting: true, size: "512", preset: "Green", set: ["count=7"] });
  assert.equal(host.committed.length, 0);
  assert.equal(ops(state, 25).length, renderCalls);

  // Anything that differs renders again: parameter, size, archive content (a new substance).
  await run(host, src, { skipExisting: true, size: "512", preset: "Green", set: ["count=8"] });
  assert.equal(host.committed.length, 1);
  host.committed.length = 0;
  await run(host, src, { skipExisting: true, size: "1024", preset: "Green", set: ["count=8"] });
  assert.equal(host.committed.length, 1);
  host.committed.length = 0;
  writeFileSync(path.join(src, "tiles.sbsar"), "a different archive");
  await run(host, src, { skipExisting: true, size: "1024", preset: "Green", set: ["count=8"] });
  assert.deepEqual(host.committed.map((entry) => entry.kind), ["plugin", "material"]);
  assert.equal(materials(host)[0].origin.derivedFrom, substanceOf(enc("a different archive")));
  // Without the flag an existing material is simply replaced.
  host.committed.length = 0;
  await run(host, src, { size: "1024", preset: "Green", set: ["count=8"] });
  assert.equal(host.committed.length, 1);
});

test("--skip-existing matches by the substance a material names: a converted material matches, an old one does not", async () => {
  const { state, src, host } = await setup(archives);
  await run(host, src, { size: "512" });
  const id = materials(host)[0].id;
  const stored = host.library.get(id);
  // A material the library converted from the old layout names its substance, qualified as the app hands ids out.
  stored.origin = { plugin: ID, derivedFrom: `lib-1::${TILES}`, meta: { sbsar: "tiles.sbsar", graph: 0, preset: null, inputs: {}, size: 9 } };
  host.committed.length = 0;
  const calls = state.calls.length;
  await run(host, src, { skipExisting: true, size: "512" });
  assert.equal(host.committed.length, 0);
  assert.equal(state.calls.length, calls);
  // One not converted yet keeps its own archive and an old fingerprint: it is rendered again, and then names the substance.
  stored.origin = { plugin: ID, files: ["library/materials/old/tiles.sbsar"], meta: { sbsar: "tiles.sbsar", graph: 0, preset: null, inputs: {}, size: 9, archiveHash: "b-0011" } };
  const result = await run(host, src, { skipExisting: true, size: "512" });
  assert.deepEqual(host.committed.map((entry) => entry.id), [id]);
  assert.deepEqual(result.replaced.map((item) => item.id), [id]);
  assert.equal(host.library.get(id).origin.derivedFrom, TILES);
  // A material whose substance was deleted is rendered again, and the substance comes back.
  host.library.delete(TILES);
  host.committed.length = 0;
  await run(host, src, { skipExisting: true, size: "512" });
  assert.deepEqual(host.committed.map((entry) => entry.kind), ["plugin", "material"]);
});

// --- rendering stored materials again ------------------------------------------

async function imported(over = {}) {
  const made = await setup(archives, over);
  await run(made.host, made.src, { size: "512", preset: "Green" });
  const entry = materials(made.host)[0];
  made.host.committed.length = 0;
  made.state.calls.length = 0;
  return { ...made, id: entry.id };
}
const render = (made, options = {}, ids) => made.host.runCommand(ID, "sbsar render", {}, options, ids ?? [made.id]);

test("tbx sbsar render updates a material in place from its substance's archive: same id, tags kept, new values recorded", async () => {
  const made = await imported();
  made.host.library.get(made.id).user.tags = ["floor"];
  const before = listTree(made.root);
  const result = await render(made, { set: ["count=9"], size: "1024" });
  assert.equal(made.host.committed.length, 1);
  const [entry] = made.host.committed;
  assert.equal(entry.id, made.id);
  assert.equal(entry.name, "tiles");
  assert.deepEqual(entry.origin, { derivedFrom: TILES, meta: { sbsar: "tiles.sbsar", graph: 0, preset: "Green", inputs: { count: 9 }, size: 10 } });
  // The archive opened is the substance's.
  assert.match(ops(made.state, 20)[0].data.substance.file, /\/library\/substance\/[^/]+\/tiles\.sbsar$/);
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
  assert.equal(made.host.library.size, 3, "the substance and two materials");
  assert.equal(created.origin.derivedFrom, TILES, "the new material is made from the same substance");
  assert.equal(created.origin.files, undefined);
  assert.equal(listTree(made.root).filter((file) => file.endsWith(".sbsar")).length, 1, "the archive is still stored once");
});

test("render --save-as on a substance makes a new material of it; without --save-as it is refused", async () => {
  const made = await imported();
  await assert.rejects(render(made, {}, [TILES]), /A substance is rendered into a new material: add --save-as NAME/);
  const dry = await render(made, { saveAs: "tiles dry", dryRun: true, preset: "Blue" }, [TILES]);
  assert.equal(dry.wouldRender[0].preset, "Blue");
  assert.equal(made.host.committed.length, 0);
  const result = await render(made, { saveAs: "tiles blue", preset: "Blue", set: ["count=5"], size: "1024" }, [TILES]);
  const [created] = made.host.committed;
  assert.deepEqual(result.created.map((item) => item.id), [created.id]);
  assert.equal(created.name, "tiles blue");
  assert.equal(created.category, "substance", "the substance's category");
  assert.deepEqual(created.origin, { derivedFrom: TILES, meta: { sbsar: "tiles.sbsar", graph: 0, preset: "Blue", inputs: { count: 5 }, size: 10 } });
  // A substance starts from its defaults (or the preset), not from any material's values.
  assert.deepEqual(updates(made.state).slice(0, 2), [["joint_height", 0.1], ["count", 5]]);
  // Saving a name the substance's materials have is refused (in any case), unless --replace; its id cannot meet an import's.
  made.host.committed.length = 0;
  const refused = await render(made, { saveAs: "Tiles BLUE" }, [TILES]);
  assert.equal(made.host.committed.length, 0);
  assert.deepEqual([refused.created, refused.replaced], [[], []]);
  assert.match(refused.skipped[0], /"Tiles BLUE" is already a material of tiles \(tiles blue, uid:[^)]+\); choose another name, or add --replace/);
  const dryTaken = await render(made, { saveAs: "tiles blue", dryRun: true }, [TILES]);
  assert.equal(dryTaken.skipped.length, 1);
  const replaced = await render(made, { saveAs: "tiles blue", replace: true }, [TILES]);
  assert.equal(made.host.committed[0].id, created.id);
  assert.deepEqual([replaced.created, replaced.replaced.map((item) => item.id)], [[], [created.id]]);
  assert.ok(made.host.logs.some((line) => line === `replaced: tiles blue (${created.id})`));
  assert.notEqual(created.id, made.id);
  assert.equal((await made.host.host.getDerivedAssets(TILES)).length, 2);
  // The imported material "tiles" (an id that follows its folder) is found by name: refused, or replaced in place.
  made.host.committed.length = 0;
  assert.match((await render(made, { saveAs: "TILES" }, [TILES])).skipped[0], /already a material of tiles \(tiles, /);
  const over = await render(made, { saveAs: "TILES", replace: true }, [TILES]);
  assert.deepEqual(over.replaced.map((item) => item.id), [made.id]);
  assert.equal(made.host.committed[0].id, made.id);
  assert.equal((await made.host.host.getDerivedAssets(TILES)).length, 2, "nothing added");
  await assert.rejects(render(made, { replace: true }), /--replace goes with --save-as/);
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

test("render skips a material whose substance is gone, whose substance lacks its archive, or that is not converted yet", async () => {
  const made = await imported();
  const substance = made.host.library.get(TILES);
  const files = substance.files;
  substance.files = [];
  const none = await render(made, {});
  assert.match(none.skipped[0], /its substance's archive \(tiles\.sbsar\) is not stored with it/);
  substance.files = files;
  made.host.library.delete(TILES);
  const gone = await render(made, {});
  assert.match(gone.skipped[0], /its substance is no longer in the library/);
  made.host.library.set(TILES, substance);
  const material = made.host.library.get(made.id);
  const link = material.origin;
  material.origin = { plugin: ID, files: ["library/materials/k/tiles.sbsar"], meta: link.meta };
  const legacy = await render(made, {});
  assert.match(legacy.skipped[0], /has not been converted to substances yet/);
  material.origin = link;
  const missing = await render(made, { set: ["nothing_like_it=1"], ignoreMissing: true });
  assert.equal(missing.updated.length, 1);
  assert.ok(made.host.warnings.some((warning) => /has no parameter "nothing_like_it"/.test(warning)));
});

test("render downloads the substance's files first where the host can", async () => {
  const made = await imported();
  const asked = [];
  made.host.host.ensureAssetFiles = async (ids) => { asked.push(...ids); };
  await render(made, {});
  assert.deepEqual(asked, [TILES]);
  made.host.host.ensureAssetFiles = async () => { throw new Error("the server is away"); };
  const failed = await render(made, {});
  assert.match(failed.skipped[0], /downloading tiles\.sbsar: the server is away/);
  delete made.host.host.ensureAssetFiles;
  assert.equal((await render(made, {})).updated.length, 1, "an older host without it: the files are local");
});

test("tbx sbsar show tells how materials were made, and what a substance holds", async () => {
  const made = await imported();
  made.host.library.set("other", { id: "other", name: "Plain", kind: "material", origin: { plugin: "x" }, user: {} });
  await render(made, { set: ["count=9"] });
  made.host.printed.length = 0;
  const shown = await made.host.runCommand(ID, "sbsar show", {}, {}, [made.id, "other", TILES]);
  assert.equal(shown.items.length, 3);
  assert.deepEqual({ ...shown.items[0], id: undefined }, {
    id: undefined, name: "tiles", library: "lib-1", substance: true, archive: "tiles.sbsar", graph: 0, preset: "Green", inputs: { count: 9 }, size: 9, pixels: 512, archiveStored: true,
    madeFrom: { id: TILES, name: "tiles" },
  });
  assert.equal(shown.items[1].substance, false);
  assert.match(made.host.printed[0], /substance: {2}tiles \(uid:\/\/substance_[0-9a-f]{32}\)[^]*archive: {4}tiles\.sbsar[^]*preset: {5}Green[^]*parameters: count=9[^]*size: {7}512 px/);
  assert.match(made.host.printed[1], /not a Substance material/);
  assert.deepEqual({ ...shown.items[2], materials: undefined }, {
    id: TILES, name: "tiles", library: "lib-1", kind: "substance", archive: "tiles.sbsar", archiveStored: true, bytes: 11, sha256: sha256(enc("archive one")),
    graphs: [{ index: 0, label: "Tiles", presets: ["Green", "Blue"], parameters: 5, outputs: ["baseColor", "normal", "roughness"] }], materials: undefined,
  });
  assert.deepEqual(shown.items[2].materials, [{ id: made.id, name: "tiles", preset: "Green", size: 9, pixels: 512 }]);
  assert.match(made.host.printed[2], /\(substance\)\n {2}archive: {4}tiles\.sbsar, 11 bytes\n {2}graph 0: {4}Tiles: 5 parameters; presets: Green, Blue[^]*materials: {2}1\n {4}tiles {2}\(Green, 512 px\)/);
  // A material whose substance is gone says so.
  made.host.library.delete(TILES);
  const orphan = await made.host.runCommand(ID, "sbsar show", {}, {}, [made.id]);
  assert.deepEqual(orphan.items[0].madeFrom, { id: TILES, missing: true });
  assert.equal(orphan.items[0].archiveStored, false);
  assert.match(made.host.printed.at(-1), /no longer in the library/);
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

test("previews release their renders; the app's scratch folder is emptied only when nothing of this session is in use", async () => {
  const { Substance, releaseRender, tidyScratch, renderBookkeeping } = await dist("lib/engine.js");
  const state = { calls: [], open: true, graphs: () => [tiles(0, "Tiles"), tiles(1, "Bricks")] };
  const services = defaultServices({ local: fakeEngine(state), settings: { get: async () => ({}), set: async () => {} } });
  const scratch = await services.scratchDir();
  // The app's fs: no removeTree.
  const { removeTree, ...appFs } = (await import("./support/cli-host.mjs")).createFs();
  const env = { services: { ...services, scratchDir: async (clear) => { if (clear) for (const name of readdirSync(scratch)) await removeTree(scratch, name); return scratch; } }, fs: appFs };
  // Other runs of this module may have released renders before: the first tidy of a session empties the folder
  // of what is left over whenever nothing is in use.
  mkdirSync(path.join(scratch, "render-left-over"), { recursive: true });
  await tidyScratch(env);
  assert.deepEqual([renders(scratch), renderBookkeeping()], [[], { unreleased: 0, released: 0, swept: true }]);
  const first = await Substance.open(env, path.join(scratch, "..", "tiles.sbsar"));
  // Another graph of the same archive is the same load: nothing is loaded again, and each graph keeps its own instance.
  const bricks = first.withGraph(1);
  assert.equal(ops(state, 20).length, 1);
  assert.equal(first.withGraph("bricks"), bricks);
  assert.equal(bricks.withGraph(0), first);
  assert.throws(() => first.withGraph(5), /has no graph 5 \(it has 0: Tiles, 1: Bricks\)/);
  const one = await bricks.render({ count: 3 }, 9);
  assert.equal(ops(state, 25).at(-1).data.substance.graph_idx, 1);
  assert.deepEqual(updates(state).at(0), ["count", 3]);
  assert.equal(renderBookkeeping().unreleased, 1);
  // A render still in use is never swept away.
  await tidyScratch(env);
  assert.deepEqual(renders(scratch), [one.folder]);
  await releaseRender(env, one);
  await releaseRender(env, one); // twice is harmless
  assert.deepEqual(renderBookkeeping(), { unreleased: 0, released: 1, swept: true });
  // Later tidies empty it only once enough released renders have piled up.
  await tidyScratch(env);
  assert.deepEqual(renders(scratch), [one.folder]);
  for (let i = 0; i < 7; i++) await releaseRender(env, await first.render({}, 9));
  await tidyScratch(env); // waits for the sweep the eighth release started
  assert.deepEqual(renders(scratch), []);
  assert.deepEqual(renderBookkeeping(), { unreleased: 0, released: 0, swept: true });
});

test("the viewer's editing steps: targets, graphs, saving as new and lists read again", async () => {
  const editor = await dist("lib/editor.js");
  const { readGraphs } = await dist("lib/graph.js");
  const [g0, g1] = readGraphs({ data: { graphs: [tiles(0, "Tiles"), tiles(1, "Bricks")] } });
  const step = (state, ...actions) => actions.reduce(editor.editReducer, state);
  const recordB = { graph: 1, preset: null, inputs: { count: 2 }, size: 9 };
  const A = { kind: "material", id: "uid://a" };
  // A material record of another graph is not laid over this graph's defaults.
  assert.deepEqual(editor.startingState(g0, recordB, 10), editor.startingState(g0, undefined, 10));
  assert.equal(editor.startingState(g1, recordB, 10).values.count, 2);

  // Choosing a material shows its own graph and starts from its record.
  let state = step(editor.initialEdit, { type: "select", target: { kind: "material", id: "uid://b" }, record: recordB });
  assert.equal(state.graph, 1);
  // A baseline of another graph (still loading) is not taken.
  assert.equal(step(state, { type: "load", baseline: editor.startingState(g0, recordB, 10) }).editor, undefined);
  state = step(state, { type: "load", baseline: editor.startingState(g1, recordB, 10) });
  assert.ok(editor.canUpdate(state));
  // Choosing another graph with a material chosen turns to a new material of that graph, from its defaults.
  const switched = step(state, { type: "graph", graph: 0 });
  assert.deepEqual([switched.target, switched.graph, switched.editor], [editor.NEW_TARGET, 0, undefined]);
  assert.ok(!editor.canUpdate(switched));
  assert.equal(step(state, { type: "graph", graph: 1 }), state, "the same graph changes nothing");

  // A material whose record cannot be read: graph 0, defaults, and its settings are said to be unknown.
  const unknown = step(state, { type: "select", target: A });
  assert.deepEqual([unknown.graph, unknown.unknown, unknown.editor], [0, true, undefined]);

  // Saved as new: the new material is the target at once, with the editor's values; "Update" waits for the list.
  const edited = step(state, { type: "edit", editor: { ...state.editor, values: { ...state.editor.values, count: 7 } } });
  const saved = step(edited, { type: "savedNew", id: "lib-1::uid://new" });
  assert.deepEqual(saved.target, { kind: "material", id: "uid://new" });
  assert.equal(saved.editor.values.count, 7);
  assert.ok(!editor.canUpdate(saved), "never Update the old material with the new one's values");
  // A list read before the new material shows keeps it the target; once listed it can be updated.
  assert.equal(step(saved, { type: "children", ids: ["lib-1::uid://b"] }).target.id, "uid://new");
  const listed = step(saved, { type: "children", ids: ["lib-1::uid://b", "lib-1::uid://new"] });
  assert.equal(listed.pending, undefined);
  assert.ok(editor.canUpdate(listed));
  // A target deleted meanwhile leaves a new material with the values kept.
  const gone = step(listed, { type: "children", ids: ["lib-1::uid://b"] });
  assert.deepEqual([gone.target, gone.editor.values.count, editor.canUpdate(gone)], [editor.NEW_TARGET, 7, false]);
  // Revert returns to the baseline for the current target.
  const reverted = step(edited, { type: "revert", baseline: editor.startingState(g1, recordB, 10) });
  assert.equal(reverted.editor.values.count, 2);

  // A new material's name may not be taken by a sibling (by cleaned name, any case), or by an item under its id.
  const { takenBy } = await dist("lib/rerender.js");
  const siblings = [{ id: "m1", name: "Brick Red", kind: "material" }, { id: "x", name: "brick blue", kind: "plugin" }];
  assert.equal(takenBy("brick red", siblings)?.id, "m1");
  assert.equal(takenBy("  BRICK RED ", siblings)?.id, "m1");
  assert.equal(takenBy("brick blue", siblings), undefined, "only materials count among siblings");
  assert.equal(takenBy("brick blue", siblings, { id: "found", name: "brick blue" })?.id, "found");
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

test("the substance module names, builds, reads and finds substances", async () => {
  const { substanceId, rawId, sameId, readSubstanceMeta, substanceEntry, substanceArchive, stageSubstanceArchive, absolutePath, isSubstance, isLegacyMaterial, hasPreview, humanBytes, summarizeGraphs } = await dist("lib/substance.js");
  const hash = "AB".repeat(32);
  assert.equal(substanceId(hash), `uid://substance_${"ab".repeat(16)}`);
  assert.throws(() => substanceId("not a hash"), /not a sha256/);
  assert.equal(rawId("lib-1::uid://substance_1"), "uid://substance_1");
  assert.equal(rawId("uid://substance_1"), "uid://substance_1");
  assert.ok(sameId("lib-1::uid://x", "uid://x") && !sameId("uid://x", "uid://y") && !sameId(undefined, "uid://x"));

  // A record read from a library is never trusted.
  for (const bad of [undefined, null, 3, [], {}, { sbsar: "" }, { sbsar: "../x.sbsar" }, { sbsar: "a\\b.sbsar" }, { sbsar: 7 }]) assert.equal(readSubstanceMeta(bad), undefined);
  assert.deepEqual(readSubstanceMeta({ sbsar: "x.sbsar" }), { sbsar: "x.sbsar", sha256: "" });
  assert.deepEqual(readSubstanceMeta({ sbsar: "x.sbsar", sha256: "nope", bytes: -4, graphs: "many" }), { sbsar: "x.sbsar", sha256: "" });
  assert.deepEqual(readSubstanceMeta({ sbsar: "x.sbsar", sha256: hash, bytes: 12.7, graphs: [{ label: "G", presets: ["a", 3], parameters: "x", outputs: ["normal"] }, null, { index: 2 }] }),
    { sbsar: "x.sbsar", sha256: hash.toLowerCase(), bytes: 12, graphs: [{ index: 0, label: "G", presets: ["a"], parameters: 0, outputs: ["normal"] }] });

  const graph = tiles(0, "Tiles");
  const { readGraphs } = await dist("lib/graph.js");
  const [read] = readGraphs({ data: { graphs: [graph] } });
  assert.deepEqual(summarizeGraphs([read]), [{ index: 0, label: "Tiles", presets: ["Green", "Blue"], parameters: 5, outputs: ["baseColor", "normal", "roughness"] }]);
  const entry = substanceEntry({ id: "uid://substance_1", pack: "_import_p", category: "c", name: "Tiles", sbsar: "t.sbsar", sha256: hash, bytes: 5, graphs: [read], preview: true });
  assert.deepEqual(entry.files, ["t.sbsar", "preview.png"]);
  assert.equal(entry.meta.graphs.length, 1);
  assert.deepEqual(substanceEntry({ id: "i", pack: "p", category: "c", name: "n", sbsar: "t.sbsar", sha256: hash, preview: false }).meta, { sbsar: "t.sbsar", sha256: hash });

  // The archive is found by its name in the record, never by position.
  const stored = { kind: "plugin", type: "substance", files: ["library/substance/k/preview.png", "library/substance/k/t.sbsar"], meta: { sbsar: "t.sbsar" } };
  assert.equal(substanceArchive(stored), "library/substance/k/t.sbsar");
  assert.equal(substanceArchive({ ...stored, meta: { sbsar: "other.sbsar" } }), undefined);
  assert.equal(substanceArchive({ files: stored.files, meta: {} }), undefined);
  assert.ok(isSubstance(stored) && !isSubstance({ kind: "material" }) && !isSubstance({ kind: "plugin", type: "skybox" }));
  assert.ok(hasPreview(stored) && !hasPreview({ files: ["a.sbsar"] }));
  assert.ok(isLegacyMaterial({ origin: { files: ["library/materials/_sources/x/a.SBSAR"] } }));
  assert.ok(!isLegacyMaterial({ origin: { files: ["library/materials/_sources/x/a.sbsar"], derivedFrom: "uid://substance_1" } }));
  assert.ok(!isLegacyMaterial({ origin: {} }));
  assert.equal(humanBytes(11), "11 bytes");
  assert.equal(humanBytes(258588243), "247 MB");
  assert.equal(humanBytes(1536), "1.5 KB");

  const src = writeTree(tempDir("a"), { "deep/x.sbsar": "data" });
  const root = tempDir("lib");
  const fs = (await import("./support/cli-host.mjs")).createFs();
  assert.equal(await stageSubstanceArchive(fs, root, "_import_p", "Tiles", { root: src, path: "deep/x.sbsar" }), "x.sbsar");
  assert.deepEqual(listTree(root), ["library/_import_p/substance/Tiles/x.sbsar"]);
  assert.equal(absolutePath({ root: "C:\\lib\\", path: "a\\b.sbsar" }), "C:/lib/a/b.sbsar");
});

test("a new material's id follows its substance and name, and never meets an import's", async () => {
  const { newMaterialId, parentOf, planRender } = await dist("lib/rerender.js");
  const { materialId } = await dist("lib/graph.js");
  const { importNamespace } = await import(pathToFileURL(path.join(pluginDir(ID), "..", "..", "packages", "toybox-sdk", "dist", "index.js")).href);
  assert.equal(newMaterialId("uid://substance_1", "Warm"), newMaterialId("lib-1::uid://substance_1", "warm"));
  assert.notEqual(newMaterialId("uid://substance_1", "Warm"), newMaterialId("uid://substance_2", "Warm"));
  assert.notEqual(newMaterialId("uid://substance_1", "tiles"), materialId(importNamespace("D:/textures"), "tiles"));
  assert.deepEqual(parentOf({ origin: { derivedFrom: "uid://substance_1" } }), { id: "uid://substance_1" });
  assert.match(parentOf({ origin: { files: ["library/materials/k/a.sbsar"] } }).reason, /not been converted/);
  assert.match(parentOf({ origin: {} }).reason, /names no substance/);
  // A substance (no record) starts from the defaults, or the preset given.
  const { readGraphs } = await dist("lib/graph.js");
  const [graph] = readGraphs({ data: { graphs: [tiles(0, "Tiles")] } });
  const loaded = { graph };
  assert.deepEqual(planRender(loaded, undefined, {}, 10), { preset: null, values: { joint_height: 0.4, glass_selection: 1, count: 12, glow: 0, tint: [0.5, 0.5, 0.5] }, size: 10, ignored: [] });
  assert.equal(planRender(loaded, undefined, { preset: "Blue" }, 10).values.count, 3);
  assert.equal(planRender(loaded, { sbsar: "t", graph: 0, preset: "Green", inputs: { count: 2 }, size: 9 }, {}).size, 9);
});

test("the editor's rules: search, groups, changed marks, starting values and new names", async () => {
  const editor = await dist("lib/editor.js");
  const { readGraphs, presetValues } = await dist("lib/graph.js");
  const raw = tiles(0, "Tiles");
  raw.inputs.push(input("hidden_one", "float", "Slider", 0.5, { visibleIf: false, guiGroup: "Joints" }), input("mortar_tint", "float", "Slider", 0.5, { guiGroup: "Mortar" }));
  const [graph] = readGraphs({ data: { graphs: [raw] } });
  const values = presetValues(graph, null);

  assert.equal(editor.prettyIdentifier("joint_height"), "Joint height");
  assert.equal(editor.prettyIdentifier("baseColorTint"), "Base color tint");
  assert.equal(editor.prettyIdentifier("$randomseed"), "Random seed");
  // Ungrouped first, then the groups in the archive's order; what the archive hides is left out.
  assert.deepEqual(editor.parameterGroups(graph, values).map((group) => [group.group, group.inputs.map((item) => item.identifier)]),
    [["", ["glass_selection", "count", "glow", "tint"]], ["Joints", ["joint_height"]], ["Mortar", ["mortar_tint"]]]);
  // A search matches labels, identifiers and group names, every word.
  assert.deepEqual(editor.parameterGroups(graph, values, "joint").map((group) => group.group), ["Joints"]);
  assert.deepEqual(editor.parameterGroups(graph, values, "mortar tint").flatMap((group) => group.inputs.map((item) => item.identifier)), ["mortar_tint"]);
  assert.deepEqual(editor.parameterGroups(graph, values, "GLASS").flatMap((group) => group.inputs.map((item) => item.identifier)), ["glass_selection"]);
  assert.deepEqual(editor.parameterGroups(graph, values, "nothing like it"), []);

  const green = presetValues(graph, "Green");
  assert.deepEqual([...editor.changedFrom({ ...green, count: 7 }, green)], ["count"]);
  assert.deepEqual([...editor.changedFrom({ ...green, tint: [0.5, 0.5, 0.5000001] }, green)], []);
  assert.deepEqual([...editor.changedFrom(green, values)], ["joint_height"]);

  // A material starts from its preset with its changes on top; a new one from the defaults.
  const start = editor.startingState(graph, editor.recordOf({ sbsar: "t.sbsar", graph: 0, preset: "Green", inputs: { count: 7, gone: 1 }, size: 9 }), 10);
  assert.deepEqual(start, { graph: 0, preset: "Green", values: { ...green, count: 7 }, size: 9 });
  assert.deepEqual(editor.startingState(graph, undefined, 11), { graph: 0, preset: null, values, size: 11 });
  assert.equal(editor.startingState(graph, { graph: 0, preset: "Withdrawn", inputs: {}, size: 9 }, 10).preset, null);
  assert.ok(editor.sameState(start, { ...start, values: { ...start.values } }));
  assert.ok(!editor.sameState(start, { ...start, size: 10 }));
  assert.ok(!editor.sameState(start, { ...start, values: { ...start.values, count: 8 } }));
  assert.equal(editor.recordOf({}), undefined);

  // Targets: the material asked for, else the first, else a new one; ids compared unqualified.
  const children = [{ id: "lib-1::uid://a", name: "b" }, { id: "lib-1::uid://b", name: "a" }];
  assert.deepEqual(editor.initialTarget(children, "uid://b"), { kind: "material", id: "uid://b" });
  assert.deepEqual(editor.initialTarget(children, "uid://gone"), { kind: "material", id: "uid://a" });
  assert.deepEqual(editor.initialTarget([], "uid://b"), { kind: "new" });
  assert.ok(editor.sameTarget({ kind: "material", id: "lib-1::uid://a" }, { kind: "material", id: "uid://a" }) && !editor.sameTarget({ kind: "new" }, { kind: "material", id: "x" }));
  assert.deepEqual(editor.sortByName([{ name: "tile 10" }, { name: "Tile 2" }, { name: "apple" }]).map((item) => item.name), ["apple", "Tile 2", "tile 10"]);
  assert.equal(editor.materialCaption({ sbsar: "t.sbsar", preset: "Green", inputs: { a: 1, b: 2 }, size: 10 }), "Green · 2 changed · 1024 px");
  assert.equal(editor.materialCaption({ sbsar: "t.sbsar", size: 9 }), "Defaults · 512 px");
  assert.equal(editor.materialCaption(undefined), "");

  // Names offered for a new material are free; a taken one is refused.
  assert.equal(editor.uniqueName("Tiles", ["tiles", "Tiles 2"]), "Tiles 3");
  assert.equal(editor.defaultNewName("Tiles", "Green", ["Tiles - Green"]), "Tiles - Green 2");
  assert.equal(editor.defaultNewName("Tiles", null, []), "Tiles");
  assert.equal(editor.uniqueName("a/b", []), "a_b");
  assert.match(editor.checkNewName("  ", []), /Enter a name/);
  assert.match(editor.checkNewName("TILES", ["tiles"]), /already a material of this substance/);
  assert.equal(editor.checkNewName("tiles warm", ["tiles"]), undefined);
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
    assert.ok(host.committed.filter((entry) => entry.kind === "material").every((entry) => entry.maps.some((map) => map.role === "albedo") && entry.origin.derivedFrom));
    assert.deepEqual(renders(scratch), []);
    void createFs;
  } finally {
    for (const child of children) child.kill();
  }
});
