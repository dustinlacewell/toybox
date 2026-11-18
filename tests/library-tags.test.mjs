import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

const load = async (file) => {
  const { outputText } = ts.transpileModule(readFileSync(new URL(`../src/domain/${file}`, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
};
const tags = await load("libraryTags.ts");
const facets = await load("facets.ts");

const user = (extra = {}) => ({ favorite: false, tags: [], ...extra });
const model = (id, extra = {}) => ({ kind: "model", library: "a", id, name: id, fileName: `${id}.gltf`, relPath: `${id}.gltf`, pack: "p", category: "props", fileset: { gltf: `${id}.gltf`, bin: `${id}.bin`, textures: [] }, animation: { clipCount: 0, clipNames: [] }, user: user(), thumb: { state: "missing" }, ...extra });

test("tags are normalised the way the library stores them", () => {
  assert.deepEqual(tags.normalizeTags(["  Foot   Step ", "footstep", "FOOTSTEP", "", "   ", "Carpet", "foot step", "\tLoop\n"]), ["foot step", "footstep", "carpet", "loop"]);
  assert.equal(tags.normalizeTag("source: Synty"), null, "a colon would pose as a provenance tag");
  assert.equal(tags.normalizeTag("a:b"), null);
  assert.equal(tags.normalizeTag("x".repeat(200)).length, tags.MAX_TAG_CHARS);
  assert.equal([...tags.normalizeTag("é".repeat(60))].length, tags.MAX_TAG_CHARS);
  assert.equal(tags.normalizeTags(Array.from({ length: 100 }, (_, n) => `tag${n}`)).length, tags.MAX_TAGS);
  assert.equal(tags.normalizeTag("bell\u0007"), "bell", "control characters are dropped");
});

test("typed tags are checked before they are sent", () => {
  assert.match(tags.tagProblem("  "), /Type a tag/);
  assert.match(tags.tagProblem("source: x"), /colon/);
  assert.equal(tags.tagProblem("coin"), null);
});

test("adding and removing a tag on a list", () => {
  assert.equal(tags.defaultTagScope, undefined, "there is no personal scope any more");
  assert.deepEqual(tags.withTag(["coin"], "  UI "), ["coin", "ui"]);
  assert.deepEqual(tags.withTag(["coin"], "Coin"), ["coin"], "already there");
  assert.deepEqual(tags.withTag(["coin"], "a:b"), ["coin"]);
  assert.deepEqual(tags.withoutTag(["coin", "ui"], "coin"), ["ui"]);
});

test("tags in the library and tags only this computer holds are told apart, and provenance tags are neither", () => {
  const asset = model("m", { tags: ["footstep", "carpet"], user: user({ tags: ["source: Synty", "mine", "mine", "license: CC0"] }) });
  assert.deepEqual(tags.libraryTags(asset), ["footstep", "carpet"]);
  assert.deepEqual(tags.personalTags(asset), ["mine"]);
  assert.deepEqual(tags.libraryTags(model("n")), []);
});

test("search and the Tags group use library and personal tags together", () => {
  const a = model("a", { tags: ["footstep"], user: user({ tags: ["mine", "source: Synty"] }) });
  const b = model("b", { tags: ["footstep", "carpet"] });
  const c = model("c", { user: user({ tags: ["carpet", "mine"] }) });
  const all = [a, b, c];
  assert.deepEqual(facets.allTags(a), ["footstep", "mine", "source: Synty"]);
  const filter = (extra) => ({ ...facets.emptyFilter("model"), ...extra });
  const names = (list) => list.map((asset) => asset.id);
  // Text search matches the name, library tags and personal tags.
  assert.deepEqual(names(facets.applyFilter(all, filter({ text: "foot" }))), ["a", "b"]);
  assert.deepEqual(names(facets.applyFilter(all, filter({ text: "MINE" }))), ["a", "c"]);
  assert.deepEqual(names(facets.applyFilter(all, filter({ text: "synty" }))), ["a"], "provenance tags still match a text search, as before");
  // A tag filter is satisfied by either kind.
  assert.deepEqual(names(facets.applyFilter(all, filter({ tags: new Set(["carpet"]) }))), ["b", "c"]);
  assert.deepEqual(names(facets.applyFilter(all, filter({ tags: new Set(["carpet", "footstep"]) }))), ["b"]);
  assert.deepEqual(names(facets.applyFilter(all, filter({ tags: new Set(["mine", "footstep"]) }))), ["a"], "one library tag and one personal tag");
  // The group counts each tag once per asset, whichever kind it is.
  const counts = Object.fromEntries(facets.tagCounts(all).map(({ value, count }) => [value, count]));
  assert.deepEqual(counts, { carpet: 2, footstep: 2, mine: 2, "source: Synty": 1 });
  const shown = facets.tagCounts(all).filter(({ value }) => !facets.isProvenanceTag(value)).map(({ value }) => value);
  assert.deepEqual(shown, ["carpet", "footstep", "mine"]);
  // The same word in both kinds on one asset is one row, one count.
  const both = model("d", { tags: ["coin"], user: user({ tags: ["coin"] }) });
  assert.deepEqual(facets.tagCounts([both]), [{ value: "coin", count: 1 }]);
});

test("an edited item replaces the old one and keeps what only exists at run time", () => {
  const old = model("a::m", { tags: ["old"], contentHash: "abc", thumb: { state: "ready", revision: 4 }, variants: [model("a::v", { thumb: { state: "ready" }, contentHash: "vhash" })] });
  const other = model("a::o");
  const catalog = { schemaVersion: 5, rigs: [], assets: [old, other] };
  const fresh = model("a::m", { tags: ["new"], variants: [model("a::v", { tags: ["lod"] })] });
  const next = tags.applyUpdatedAssets(catalog, [fresh]);
  assert.notEqual(next, catalog);
  assert.deepEqual(next.assets.map((asset) => asset.id), ["a::m", "a::o"]);
  assert.equal(next.assets[1], other, "untouched items keep their identity");
  assert.deepEqual(next.assets[0].tags, ["new"]);
  assert.deepEqual(next.assets[0].thumb, { state: "ready", revision: 4 });
  assert.equal(next.assets[0].contentHash, "abc");
  assert.deepEqual(next.assets[0].variants[0].tags, ["lod"]);
  assert.equal(next.assets[0].variants[0].contentHash, "vhash");
  assert.deepEqual(next.assets[0].variants[0].thumb, { state: "ready" });
  assert.equal(tags.applyUpdatedAssets(catalog, [model("a::zzz")]), catalog, "nothing to replace, nothing copied");
  assert.equal(tags.applyUpdatedAssets(null, [fresh]), null);
});
