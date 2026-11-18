import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
const load = async (file) => {
  const { outputText } = ts.transpileModule(readFileSync(new URL(`../src/domain/${file}`, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
};
const ids = await load("libraryId.ts");
const facets = await load("facets.ts");

const user = (extra = {}) => ({ favorite: false, tags: [], ...extra });
const model = (id, extra = {}) => ({ kind: "model", id, name: id, fileName: `${id}.gltf`, relPath: `${id}.gltf`, pack: "p", category: "props", fileset: { gltf: `${id}.gltf`, bin: `${id}.bin`, textures: [] }, animation: { clipCount: 0, clipNames: [] }, user: user(), thumb: { state: "missing" }, ...extra });
const clip = (id, sourceId, extra = {}) => ({ kind: "animation", id, name: id, fileName: "a.gltf", relPath: "a.gltf", pack: "p", category: "idle", fileset: { gltf: "a.gltf", bin: "a.bin", textures: [] }, clip: { index: 0, name: "Idle", duration: 1, sourceId, sourceName: "Hero", targetNodes: [], rig: "r" }, user: user(), thumb: { state: "missing" }, ...extra });

test("qualified ids split on the first separator, whatever the raw id holds", () => {
  assert.equal(ids.qualify("ab12", "uid://x"), "ab12::uid://x");
  assert.deepEqual(ids.parseQualified("ab12::uid://import_p::odd"), { library: "ab12", id: "uid://import_p::odd" });
  assert.equal(ids.parseQualified("uid://x"), null);
  assert.equal(ids.parseQualified("toybox:skeleton"), null);
  assert.equal(ids.parseQualified("::x"), null);
  assert.equal(ids.libraryOf("ab12::x"), "ab12");
  assert.throws(() => ids.libraryOf("raw"), /Not a library item id/);
  assert.equal(ids.unqualify("ab12", "ab12::x"), "x");
  assert.equal(ids.unqualify("ab12", "cd34::x"), "cd34::x", "another library prefix is not stripped");
  assert.equal(ids.unqualify("ab12", "toybox:skeleton"), "toybox:skeleton");
});

test("an asset id, its variants and every reference to another asset are qualified, built-ins are not", () => {
  const asset = model("uid://m", { variants: [model("uid://m_lod1", { user: user({ previewAssetId: "uid://anim" }) })], user: user({ previewAssetId: "uid://anim" }) });
  const q = ids.qualifyAsset(asset, "aaa");
  assert.equal(q.id, "aaa::uid://m");
  assert.equal(q.library, "aaa");
  assert.equal(q.user.previewAssetId, "aaa::uid://anim");
  assert.equal(q.variants[0].id, "aaa::uid://m_lod1");
  assert.equal(q.variants[0].library, "aaa");
  assert.equal(q.variants[0].user.previewAssetId, "aaa::uid://anim");
  const a = ids.qualifyAsset(clip("uid://anim_1_0", "uid://m", { user: user({ previewAssetId: "toybox:skeleton" }) }), "aaa");
  assert.equal(a.clip.sourceId, "aaa::uid://m");
  assert.equal(a.user.previewAssetId, "toybox:skeleton", "built-in preview sentinels stay as they are");
  assert.equal(asset.id, "uid://m", "the input is not mutated");
  // Qualifying twice is a no-op for references already qualified.
  assert.equal(ids.qualifyAsset(a, "aaa").clip.sourceId, "aaa::uid://m");
});

test("un-qualifying restores the raw asset exactly, with no library stamp", () => {
  const asset = model("uid://m", { variants: [model("uid://v")], user: user({ previewAssetId: "uid://anim", favorite: true, tags: ["rock"] }) });
  assert.deepEqual(ids.unqualifyAsset(ids.qualifyAsset(asset, "aaa"), "aaa"), asset);
  const a = clip("uid://c", "uid://m");
  assert.deepEqual(ids.unqualifyAsset(ids.qualifyAsset(a, "aaa"), "aaa"), a);
});

const raw = (root, assets, rigs = []) => ({ schemaVersion: 5, libraryRoot: root, rigs, assets });

test("two libraries with the same raw id stay distinct in the merged catalog and split back apart", () => {
  const a = ids.qualifyCatalog(raw("D:/a", [model("uid://same", { user: user({ favorite: true }) })], [{ id: "rig1" }]), "aaa");
  const b = ids.qualifyCatalog(raw("E:/b", [model("uid://same", { user: user({ tags: ["b-only"] }) })], [{ id: "rig1" }, { id: "rig2" }]), "bbb");
  assert.equal("libraryRoot" in a, false, "the merged form has no single root");
  const merged = ids.mergeCatalogs([{ library: "aaa", catalog: a }, { library: "bbb", catalog: b }]);
  assert.deepEqual(merged.assets.map((x) => x.id), ["aaa::uid://same", "bbb::uid://same"]);
  assert.deepEqual(merged.rigs.map((r) => r.id), ["rig1", "rig2"], "equal rig ids are one rig");
  const parts = ids.splitCatalog(merged);
  assert.deepEqual([...parts.keys()], ["aaa", "bbb"]);
  assert.equal(parts.get("aaa").assets[0].id, "uid://same");
  assert.equal(parts.get("aaa").assets[0].user.favorite, true);
  assert.deepEqual(parts.get("bbb").assets[0].user.tags, ["b-only"]);
  // Nothing written back carries a qualified id or the stamp.
  const written = JSON.stringify([...parts.values()]);
  assert.ok(!written.includes("::"), written);
  assert.ok(!written.includes('"library"'));
});

test("a library with no assets in the merged catalog is never written back", () => {
  const merged = ids.mergeCatalogs([{ library: "aaa", catalog: ids.qualifyCatalog(raw("D:/a", [model("x")]), "aaa") }]);
  assert.deepEqual([...ids.splitCatalog(merged).keys()], ["aaa"]);
  assert.equal(ids.splitCatalog({ schemaVersion: 5, rigs: [], assets: [] }).size, 0);
});

test("replacing, dropping and scoping one library slice leaves the others alone", () => {
  const a = ids.qualifyCatalog(raw("D:/a", [model("1"), model("2")]), "aaa");
  const b = ids.qualifyCatalog(raw("E:/b", [model("1")]), "bbb");
  const merged = ids.mergeCatalogs([{ library: "aaa", catalog: a }, { library: "bbb", catalog: b }]);
  const fresh = ids.qualifyCatalog(raw("D:/a", [model("3")]), "aaa");
  const replaced = ids.replaceLibrary(merged, "aaa", fresh, ["aaa", "bbb"]);
  assert.deepEqual(replaced.assets.map((x) => x.id), ["aaa::3", "bbb::1"]);
  assert.deepEqual(ids.replaceLibrary(null, "bbb", b, ["aaa", "bbb"]).assets.map((x) => x.id), ["bbb::1"]);
  assert.deepEqual(ids.dropLibrary(merged, "aaa").assets.map((x) => x.id), ["bbb::1"]);
  const scoped = ids.scopeCatalog(merged, "bbb");
  assert.deepEqual(scoped.assets.map((x) => x.id), ["bbb::1"]);
  assert.equal(ids.scopeCatalog(merged, "bbb"), scoped, "memoised for a stable identity");
  assert.equal(ids.scopeCatalog(null, "bbb"), null);
});

test("ids group by library as raw ids in first-seen order, and the first item names the scope", () => {
  const groups = ids.groupIds(["bbb::x", "aaa::y", "bbb::z", "aaa::uid://q"]);
  assert.deepEqual([...groups], [["bbb", ["x", "z"]], ["aaa", ["y", "uid://q"]]]);
  assert.throws(() => ids.groupIds(["bbb::x", "plain"]), /Not a library item id: plain/);
  assert.equal(ids.scopeLibrary(["bbb::x", "aaa::y"], "def"), "bbb");
  assert.equal(ids.scopeLibrary([], "def"), "def");
  assert.deepEqual(ids.qualifyAll("aaa", ["x", "y"]), ["aaa::x", "aaa::y"]);
  assert.deepEqual(ids.qualifyKeys("aaa", { x: 1 }), { "aaa::x": 1 });
});

test("entries a plugin built with a qualified id are written with raw ids", () => {
  const entry = { id: "aaa::uid://mat", kind: "material", variants: [{ id: "aaa::uid://mat_1k" }, { id: "uid://other" }] };
  const out = ids.unqualifyEntry("aaa", entry);
  assert.equal(out.id, "uid://mat");
  assert.deepEqual(out.variants.map((v) => v.id), ["uid://mat_1k", "uid://other"]);
  assert.equal(entry.id, "aaa::uid://mat");
});

test("export reports from several libraries merge and flag files that overwrote each other", () => {
  const one = { written: ["Tree.glb", "Rock.glb"], skipped: ["missing: x"], warnings: [] };
  const two = { written: ["Tree.glb", "Bush.glb"], skipped: [], warnings: ["note"] };
  const merged = ids.mergeReports([one, two]);
  assert.deepEqual(merged.written, ["Tree.glb", "Rock.glb", "Tree.glb", "Bush.glb"]);
  assert.deepEqual(merged.skipped, ["missing: x"]);
  assert.equal(merged.warnings[0], "note");
  assert.match(merged.warnings[1], /1 file was written by more than one library/);
  assert.equal(ids.mergeReports([one]).warnings.length, 0);
});

test("the Libraries filter narrows by library, counts live, and is part of filter-active", () => {
  const assets = [model("1", { library: "aaa" }), model("2", { library: "aaa" }), model("1", { library: "bbb" })];
  const filter = facets.emptyFilter("model");
  assert.equal(facets.isFilterActive(filter), false);
  assert.equal(facets.applyFilter(assets, filter).length, 3);
  const only = { ...filter, libraries: new Set(["bbb"]) };
  assert.equal(facets.isFilterActive(only), true);
  assert.deepEqual(facets.applyFilter(assets, only).map((a) => a.library), ["bbb"]);
  assert.deepEqual(facets.libraryCounts(assets), [{ value: "aaa", count: 2 }, { value: "bbb", count: 1 }]);
  assert.equal(facets.applyFilter(assets, { ...filter, libraries: new Set(["aaa", "bbb"]) }).length, 3);
});
