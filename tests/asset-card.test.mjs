import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

function moduleUrl(file, replacements = {}) {
  let { outputText } = ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  for (const [from, to] of Object.entries(replacements)) outputText = outputText.replaceAll(`from "${from}"`, `from ${JSON.stringify(to)}`);
  return `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
}
const facetsUrl = moduleUrl("../src/domain/facets.ts");
const { assetStats, assetFacets, tagSuggestions, rigNodeCount } = await import(moduleUrl("../src/domain/assetCard.ts", {
  "./facets": facetsUrl, "@ldlework/toybox-sdk": import.meta.resolve("@ldlework/toybox-sdk"),
}));

const format = { time: (seconds) => `t${seconds}` };
const base = (kind, extra = {}) => ({
  id: "a", name: "Crate_01", kind, category: "props", library: "lib", tags: [], fileName: "x", relPath: "", pack: "p",
  thumb: { state: "missing" }, user: { favorite: false, tags: [] }, ...extra,
});
const tiles = (stats) => stats.map(({ caption, value, press }) => press ? `${caption}=${value}>${press}` : `${caption}=${value}`);
const sane = (stats) => {
  assert.ok(stats.length <= 4);
  for (const stat of stats) assert.notEqual(stat.value, "");
  assert.equal(new Set(stats.map((stat) => stat.id)).size, stats.length);
};

test("a model shows its detail levels, and its rig and origin when it has them", () => {
  const model = base("model", { format: "fbx" });
  const full = assetStats({ asset: model, variants: [model, model, model], rigNodes: 42 }, format);
  sane(full);
  assert.deepEqual(tiles(full), ["Detail levels=3>open", "Bones=42", "Origin=FBX"]);
  const bare = assetStats({ asset: base("model"), variants: [base("model")] }, format);
  assert.deepEqual(tiles(bare), [], "a single detail level is not a readout");
});

test("a material shows maps, sizes and its largest size", () => {
  const material = base("material", { name: "Wood_4K", material: { maps: [{}, {}, {}] } });
  const stats = assetStats({ asset: material, variants: [material, material] }, format);
  sane(stats);
  assert.equal(stats.length, 3);
  assert.deepEqual(tiles(stats).slice(0, 2), ["Maps=3>open", "Sizes=2>open"]);
  assert.equal(stats[2].caption, "Largest size");
});

test("audio shows length, sample rate, channels and format in the full view's wording", () => {
  const audio = base("audio", { audio: { file: "x.wav", format: "wav", duration: 3, sampleRate: 48000, channels: 2 } });
  const stats = assetStats({ asset: audio, variants: [audio] }, format);
  sane(stats);
  assert.deepEqual(tiles(stats), ["Length=t3>open", "Sample rate=48 kHz", "Channels=Stereo", "Format=wav"]);
});

test("an animation shows length, rig, animated bones and its source clip", () => {
  const clip = { duration: 1.5, rig: "Humanoid", targetNodes: ["a", "b"], sourceName: "Walk.fbx" };
  const stats = assetStats({ asset: base("animation", { clip }), variants: [] }, format);
  sane(stats);
  assert.deepEqual(tiles(stats), ["Length=1.50 s>open", "Rig=Humanoid", "Animated bones=2", "Source clip=Walk.fbx>open"]);
});

test("a plugin type shows its first three rows and its file count", () => {
  const plugin = base("plugin", { type: "x.font", files: ["a", "b"], meta: {} });
  const properties = [1, 2, 3, 4, 5].map((n) => ({ label: `P${n}`, value: n === 2 ? `v${n}` : n }));
  const stats = assetStats({ asset: plugin, variants: [plugin], properties }, format);
  sane(stats);
  assert.deepEqual(tiles(stats), ["P1=1", "P2=v2", "P3=3", "Files=2"]);
  assert.deepEqual(tiles(assetStats({ asset: plugin, variants: [plugin] }, format)), ["Files=2"]);
  // An empty value is no tile.
  assert.deepEqual(tiles(assetStats({ asset: plugin, variants: [plugin], properties: [{ label: "E", value: "" }, { label: "F", value: 0 }] }, format)), ["F=0", "Files=2"]);
});

test("the rig node count comes from the first known rig", () => {
  const rigs = [{ id: "r1", nodes: [1, 2] }, { id: "r2", nodes: [1, 2, 3] }];
  assert.equal(rigNodeCount([{ rigId: "missing" }, { rigId: "r2" }], rigs), 3);
  assert.equal(rigNodeCount(undefined, rigs), undefined);
  assert.equal(rigNodeCount([{ rigId: "r1" }], undefined), undefined);
});

test("the values that filter read publisher, source, license, category, each labeled, and skip empty ones", () => {
  const full = base("audio", { user: { favorite: false, tags: [], provenance: { source: "Nature Kit", publisher: "Kenney", license: "CC0" } } });
  assert.deepEqual(assetFacets(full), [
    { field: "publishers", label: "Publisher", value: "Kenney" }, { field: "sources", label: "Source", value: "Nature Kit" },
    { field: "licenses", label: "License", value: "CC0" }, { field: "categories", label: "Category", value: "props" },
  ]);
  const some = base("audio", { category: "", user: { favorite: false, tags: ["license: MIT"], provenance: { source: "Pack" } } });
  assert.deepEqual(assetFacets(some), [{ field: "sources", label: "Source", value: "Pack" }, { field: "licenses", label: "License", value: "MIT" }]);
});

test("tag suggestions leave out the item's tags in any case, keep their order and stop at the limit", () => {
  const found = ["Wood", "crate", "prop", "barrel", "box", "metal", "rust", "old", "new", "red"].map((value, count) => ({ value, count }));
  assert.deepEqual(tagSuggestions(found, ["wood", "PROP"]).map((row) => row.value), ["crate", "barrel", "box", "metal", "rust", "old", "new", "red"]);
  assert.equal(tagSuggestions(found, []).length, 8);
  assert.deepEqual(tagSuggestions(found, [], 2).map((row) => row.value), ["Wood", "crate"]);
  assert.deepEqual(tagSuggestions(undefined, ["x"]), []);
});
