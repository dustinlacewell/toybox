import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
const load = async (file) => {
  const { outputText } = ts.transpileModule(readFileSync(new URL(`../src/domain/${file}`, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
};
const dup = await load("duplicates.ts");
const ids = await load("libraryId.ts");
const facets = await load("facets.ts");

const user = (extra = {}) => ({ favorite: false, tags: [], ...extra });
const model = (library, raw, extra = {}) => ({ kind: "model", id: `${library}::${raw}`, library, name: raw, fileName: `${raw}.gltf`, relPath: `${raw}.gltf`, pack: "p", category: "props", fileset: { gltf: `${raw}.gltf`, bin: `${raw}.bin`, textures: [] }, animation: { clipCount: 0, clipNames: [] }, user: user(), thumb: { state: "missing" }, ...extra });
const audio = (library, raw, extra = {}) => ({ kind: "audio", id: `${library}::${raw}`, library, name: raw, fileName: `${raw}.wav`, relPath: `${raw}.wav`, pack: "p", category: "sfx", audio: { file: `${raw}.wav` }, user: user(), thumb: { state: "missing" }, ...extra });
const plugin = (library, raw, type) => ({ kind: "plugin", type, id: `${library}::${raw}`, library, name: raw, fileName: raw, relPath: raw, pack: "p", category: "c", files: ["f"], user: user(), thumb: { state: "missing" } });

const libs = [
  { id: "loc1", kind: "local", isDefault: false, available: true },
  { id: "loc2", kind: "local", isDefault: true, available: true },
  { id: "loc3", kind: "local", isDefault: false, available: true },
  { id: "rem1", kind: "remote", isDefault: false, available: true },
];
const ranks = dup.libraryRanks(libs);
const idsOf = (list) => list.map((a) => a.id);

test("items group by kind and raw id: the same raw id of another kind is a different item", () => {
  const a = [model("loc1", "x"), audio("loc1", "x"), model("loc3", "x"), plugin("loc1", "x", "decal"), plugin("loc3", "x", "decal"), plugin("loc3", "y", "decal")];
  const out = dup.dedupeAssets(a, ranks);
  assert.ok(out.assets.length === 4 && out.assets[0] === a[0] && out.assets[1] === a[1] && out.assets[2] === a[3] && out.assets[3] === a[5]);
});

test("preference: default library, then other locals in registry order, then remote", () => {
  const all = [model("rem1", "m"), model("loc3", "m"), model("loc1", "m"), model("loc2", "m")];
  assert.deepEqual(idsOf(dup.dedupeAssets(all, ranks).assets), ["loc2::m"]);
  assert.deepEqual(idsOf(dup.dedupeAssets(all.filter((a) => a.library !== "loc2"), ranks).assets), ["loc1::m"]);
  assert.deepEqual(idsOf(dup.dedupeAssets([all[0], all[1]], ranks).assets), ["loc3::m"]);
  assert.deepEqual(idsOf(dup.dedupeAssets([all[0]], ranks).assets), ["rem1::m"]);
});

test("an offline or unavailable library loses to an available one, even the default or a local", () => {
  const down = dup.libraryRanks([
    { id: "loc1", kind: "local", isDefault: true, available: false },
    { id: "loc2", kind: "local", available: true },
    { id: "rem1", kind: "remote", available: true, remote: { offline: false } },
    { id: "rem2", kind: "remote", available: true, offline: true },
  ]);
  assert.deepEqual(idsOf(dup.dedupeAssets([model("loc1", "m"), model("rem1", "m")], down).assets), ["rem1::m"], "an available remote beats an unplugged default");
  assert.deepEqual(idsOf(dup.dedupeAssets([model("rem2", "m"), model("loc2", "m")], down).assets), ["loc2::m"]);
  assert.deepEqual(idsOf(dup.dedupeAssets([model("rem2", "m"), model("loc1", "m")], down).assets), ["loc1::m"], "when both are down, the usual order decides");
});

test("the representative lists the other copies, best first, from any copy; order and objects are kept", () => {
  const a = model("rem1", "m"), b = model("loc1", "m"), c = model("loc2", "n"), d = model("loc3", "m");
  const out = dup.dedupeAssets([a, b, c, d], ranks);
  assert.deepEqual(idsOf(out.assets), ["loc1::m", "loc2::n"], "input order");
  assert.ok(out.assets[0] === b && out.assets[1] === c, "the same objects, not copies");
  assert.deepEqual(out.alsoIn.get("loc1::m"), [{ library: "loc3", id: "loc3::m" }, { library: "rem1", id: "rem1::m" }]);
  assert.deepEqual(out.alsoIn.get("rem1::m").map((x) => x.library), ["loc1", "loc3"]);
  assert.equal(out.alsoIn.has("loc2::n"), false, "an item that exists once has no entry");
  assert.deepEqual(dup.librariesHolding(b, out.alsoIn), ["loc1", "loc3", "rem1"]);
  assert.deepEqual(dup.librariesHolding(c, out.alsoIn), ["loc2"]);
});

test("the Libraries filter shows the selected libraries own copies, de-duplicated among them", () => {
  const all = [model("loc1", "m"), model("loc2", "m"), model("rem1", "m"), model("rem1", "only-remote")];
  assert.deepEqual(idsOf(dup.dedupeAssets(all, ranks).assets), ["loc2::m", "rem1::only-remote"]);
  assert.deepEqual(idsOf(dup.dedupeAssets(all, ranks, new Set(["rem1"])).assets), ["rem1::m", "rem1::only-remote"]);
  assert.deepEqual(idsOf(dup.dedupeAssets(all, ranks, new Set(["loc1", "rem1"])).assets), ["loc1::m", "rem1::only-remote"]);
  assert.deepEqual(idsOf(dup.dedupeAssets(all, ranks, new Set()).assets), ["loc2::m", "rem1::only-remote"], "an empty set means every library");
  // The other copies are still reported, whichever libraries are selected.
  assert.deepEqual(dup.dedupeAssets(all, ranks, new Set(["rem1"])).alsoIn.get("rem1::m").map((x) => x.library), ["loc2", "loc1"].sort((x, y) => ranks.get(x) - ranks.get(y)));
});

test("counts: facets count a duplicated item once, the Libraries group counts what each library holds", () => {
  const a = (library, raw, extra) => model(library, raw, extra);
  const all = [
    a("loc1", "m", { user: user({ tags: ["rock"], provenance: { source: "Synty" } }), category: "props" }),
    a("loc2", "m", { user: user({ tags: ["rock"], provenance: { source: "Synty" } }), category: "props" }),
    a("loc2", "n", { category: "trees" }),
    audio("loc2", "s"),
  ];
  const unique = dup.dedupeAssets(all, ranks).assets;
  const kind = facets.ofKind(unique, "model");
  assert.equal(kind.length, 2, "sidebar/toolbar total");
  assert.equal(facets.ofKind(all, "model").length, 3);
  assert.deepEqual(facets.categoryCounts(kind), [{ value: "props", count: 1 }, { value: "trees", count: 1 }]);
  assert.deepEqual(facets.provenanceCounts(kind, "source"), [{ value: "", count: 1 }, { value: "Synty", count: 1 }]);
  assert.deepEqual(facets.tagCounts(kind), [{ value: "rock", count: 1 }]);
  assert.deepEqual(facets.formatCounts(kind), [{ value: "gltf", count: 2 }]);
  assert.deepEqual(facets.libraryCounts(facets.ofKind(all, "model")), [{ value: "loc1", count: 1 }, { value: "loc2", count: 2 }], "true per-library counts");
  // With a library selected, the grid is that library's copies.
  const filter = { ...facets.emptyFilter("model"), libraries: new Set(["loc1"]) };
  const shown = facets.applyFilter(dup.dedupeAssets(all, ranks, filter.libraries).assets, filter);
  assert.deepEqual(idsOf(shown), ["loc1::m"]);
});

test("copies already in a target library are detected for the transfer dialog", () => {
  const a = model("loc1", "m"), b = model("loc2", "m"), c = model("loc1", "n");
  const { alsoIn } = dup.dedupeAssets([a, b, c], ranks);
  assert.equal(dup.allThere([a], alsoIn, "loc2"), true);
  assert.equal(dup.allThere([a, c], alsoIn, "loc2"), false, "one of them is missing there");
  assert.equal(dup.allThere([a, c], alsoIn, "loc1"), true, "its own library has it");
  assert.equal(dup.allThere([], alsoIn, "loc2"), false);
  assert.equal(dup.allThere([a], alsoIn, "rem1"), false);
});

test("nothing derived reaches what is saved", () => {
  const merged = ids.mergeCatalogs([
    { library: "loc1", catalog: ids.qualifyCatalog({ schemaVersion: 5, libraryRoot: "D:/a", rigs: [], assets: [{ ...model("x", "m"), id: "m", library: undefined }] }, "loc1") },
    { library: "loc2", catalog: ids.qualifyCatalog({ schemaVersion: 5, libraryRoot: "E:/b", rigs: [], assets: [{ ...model("x", "m"), id: "m", library: undefined }] }, "loc2") },
  ]);
  const before = JSON.stringify(merged);
  const out = dup.dedupeAssets(merged.assets, ranks);
  assert.equal(JSON.stringify(merged), before, "the merged catalog is not touched");
  assert.ok(!JSON.stringify(out.assets).includes("alsoIn"), "no derived field on an asset");
  const parts = ids.splitCatalog(merged);
  assert.deepEqual([...parts.keys()], ["loc1", "loc2"], "both copies are still written, each to its own library");
  assert.ok(!JSON.stringify([...parts.values()]).includes("alsoIn"));
  assert.equal(parts.get("loc1").assets[0].id, "m");
});

test("20,000 assets de-duplicate in linear time", () => {
  const all = [];
  for (let i = 0; i < 10_000; i++) { all.push(model("loc1", `a${i}`)); all.push(model(i % 2 ? "loc2" : "rem1", `a${i}`)); }
  const started = performance.now();
  const out = dup.dedupeAssets(all, ranks);
  const scoped = dup.dedupeAssets(all, ranks, new Set(["rem1", "loc2"]));
  const elapsed = performance.now() - started;
  assert.equal(out.assets.length, 10_000);
  assert.equal(scoped.assets.length, 10_000);
  assert.equal(out.alsoIn.size, 20_000);
  assert.ok(elapsed < 2_000, `took ${elapsed} ms`);
});

// ---- by content ---------------------------------------------------------------------------------

const H1 = "a".repeat(64), H2 = "b".repeat(64), H3 = "c".repeat(64);
const hashed = (asset, contentHash) => ({ ...asset, contentHash });

test("the same content under different ids in different libraries is one item", () => {
  const a = hashed(model("loc1", "Crate_01"), H1), b = hashed(model("loc3", "uid://import_other_crate"), H1), c = hashed(model("rem1", "x"), H1);
  const out = dup.dedupeAssets([c, b, a], ranks);
  assert.deepEqual(idsOf(out.assets), ["loc1::Crate_01"], "the preferred library's copy");
  assert.deepEqual(out.alsoIn.get("loc1::Crate_01").map((x) => x.library), ["loc3", "rem1"]);
  assert.deepEqual(out.alsoIn.get("rem1::x").map((x) => x.library), ["loc1", "loc3"], "every copy knows the others");
  assert.equal(out.otherVersions.size, 0);
  assert.equal(dup.allThere([a], out.alsoIn, "rem1"), true, "the transfer dialog sees it as already there");
  // Another type with the same bytes is another item.
  const sound = hashed(audio("loc3", "c"), H1);
  assert.deepEqual(idsOf(dup.dedupeAssets([a, sound], ranks).assets), ["loc1::Crate_01", "loc3::c"]);
});

test("the same id with different content is two items, and each says there is another version", () => {
  const a = hashed(model("loc1", "m"), H1), b = hashed(model("loc2", "m"), H2), c = hashed(model("rem1", "m"), H1);
  const out = dup.dedupeAssets([a, b, c], ranks);
  assert.deepEqual(idsOf(out.assets), ["loc1::m", "loc2::m"], "the remote copy equals loc1's; loc2's is different");
  assert.deepEqual(out.alsoIn.get("loc1::m"), [{ library: "rem1", id: "rem1::m" }]);
  assert.equal(out.alsoIn.has("loc2::m"), false);
  assert.deepEqual(out.otherVersions.get("loc1::m"), [{ library: "loc2", id: "loc2::m" }]);
  assert.deepEqual(out.otherVersions.get("loc2::m").map((x) => x.library), ["loc1", "rem1"]);
  assert.deepEqual(out.otherVersions.get("rem1::m"), [{ library: "loc2", id: "loc2::m" }]);
  // Only when both fingerprints are known.
  assert.equal(dup.dedupeAssets([a, model("loc2", "m")], ranks).otherVersions.size, 0);
});

test("identical content inside one library is never collapsed", () => {
  const kit = ["Wall_A", "Wall_B", "Wall_C"].map((raw) => hashed(model("loc1", raw), H1));
  const out = dup.dedupeAssets(kit, ranks);
  assert.deepEqual(idsOf(out.assets), ["loc1::Wall_A", "loc1::Wall_B", "loc1::Wall_C"]);
  assert.equal(out.alsoIn.size, 0);
  // A copy of the kit in another library hides once per item, not the whole kit into one.
  const second = ["Wall_A", "Wall_B", "Wall_C"].map((raw) => hashed(model("rem1", raw), H1));
  const both = dup.dedupeAssets([...kit, ...second], ranks);
  assert.deepEqual(idsOf(both.assets), ["loc1::Wall_A", "loc1::Wall_B", "loc1::Wall_C"]);
  // Each lower copy is listed under the item with its own raw id.
  assert.deepEqual(both.alsoIn.get("loc1::Wall_B").map((x) => x.id), ["rem1::Wall_B"]);
  assert.deepEqual(both.alsoIn.get("loc1::Wall_C").map((x) => x.id), ["rem1::Wall_C"]);
  // With no matching raw id it goes under the first.
  const renamed = dup.dedupeAssets([...kit, hashed(model("rem1", "Other"), H1)], ranks);
  assert.deepEqual(renamed.alsoIn.get("loc1::Wall_A").map((x) => x.id), ["rem1::Other"]);
  assert.equal(renamed.alsoIn.has("loc1::Wall_B"), false);
  assert.deepEqual(idsOf(renamed.assets), ["loc1::Wall_A", "loc1::Wall_B", "loc1::Wall_C"]);
  // Two identical items of the lower library both hide (each into the higher library's).
  const twice = dup.dedupeAssets([hashed(model("loc1", "One"), H2), hashed(model("rem1", "P"), H2), hashed(model("rem1", "Q"), H2)], ranks);
  assert.deepEqual(idsOf(twice.assets), ["loc1::One"]);
  assert.equal(twice.alsoIn.get("loc1::One").length, 2);
});

test("a pair with an unfingerprinted side falls back to type and raw id", () => {
  // Hashed vs unhashed: the same raw id is the same item; another raw id is not (nothing says it is).
  const same = dup.dedupeAssets([hashed(model("loc1", "m"), H1), model("rem1", "m")], ranks);
  assert.deepEqual(idsOf(same.assets), ["loc1::m"]);
  const other = dup.dedupeAssets([hashed(model("loc1", "m"), H1), model("rem1", "n")], ranks);
  assert.deepEqual(idsOf(other.assets), ["loc1::m", "rem1::n"]);
  // Unhashed vs hashed, the other way round.
  assert.deepEqual(idsOf(dup.dedupeAssets([model("loc1", "m"), hashed(model("rem1", "m"), H1)], ranks).assets), ["loc1::m"]);
  assert.deepEqual(idsOf(dup.dedupeAssets([model("loc1", "m"), hashed(model("rem1", "n"), H1)], ranks).assets), ["loc1::m", "rem1::n"]);
  // An unhashed pair is the old rule.
  assert.deepEqual(idsOf(dup.dedupeAssets([model("loc1", "m"), model("rem1", "m")], ranks).assets), ["loc1::m"]);
  // A mixed library: hashed items match by content, the rest by id, in one pass.
  const mixed = dup.dedupeAssets([
    hashed(model("loc1", "a"), H1), model("loc1", "b"), hashed(model("loc1", "c"), H3),
    hashed(model("rem1", "renamed_a"), H1), model("rem1", "b"), model("rem1", "c"), hashed(model("rem1", "c2"), H2),
  ], ranks);
  assert.deepEqual(idsOf(mixed.assets), ["loc1::a", "loc1::b", "loc1::c", "rem1::c2"]);
});

test("fingerprints that arrive later regroup the items, with no rescan", () => {
  const merged = { schemaVersion: 5, rigs: [], assets: [model("loc1", "Crate"), model("loc3", "uid://crate_copy"), model("rem1", "Crate")] };
  const first = dup.dedupeAssets(merged.assets, ranks);
  assert.deepEqual(idsOf(first.assets), ["loc1::Crate", "loc3::uid://crate_copy"], "by id: the renamed copy is a different item");
  // loc3 is fingerprinted (raw ids, as the backend sends them).
  const step1 = ids.patchContentHashes(merged, "loc3", { "uid://crate_copy": H1 });
  assert.notEqual(step1, merged);
  assert.ok(step1.assets[0] === merged.assets[0] && step1.assets[2] === merged.assets[2], "other items keep their identity");
  assert.equal(step1.assets[1].contentHash, H1);
  assert.equal(merged.assets[1].contentHash, undefined, "the old catalog is not mutated");
  assert.deepEqual(idsOf(dup.dedupeAssets(step1.assets, ranks).assets), ["loc1::Crate", "loc3::uid://crate_copy"], "one side only: still the old rule");
  // loc1 follows: both hashed and equal, so the renamed copy collapses; rem1 is still unhashed and matches loc1 by id.
  const step2 = ids.patchContentHashes(step1, "loc1", { Crate: H1 });
  const out = dup.dedupeAssets(step2.assets, ranks);
  assert.deepEqual(idsOf(out.assets), ["loc1::Crate"]);
  assert.deepEqual(out.alsoIn.get("loc1::Crate").map((x) => x.library), ["loc3", "rem1"]);
  // rem1 then turns out to hold a different version under the same id.
  const step3 = ids.patchContentHashes(step2, "rem1", { Crate: H2 });
  const last = dup.dedupeAssets(step3.assets, ranks);
  assert.deepEqual(idsOf(last.assets), ["loc1::Crate", "rem1::Crate"]);
  assert.deepEqual(last.otherVersions.get("loc1::Crate").map((x) => x.library), ["rem1"]);
  // Nothing to change: the very same catalog comes back.
  assert.equal(ids.patchContentHashes(step3, "loc1", { Crate: H1 }), step3);
  assert.equal(ids.patchContentHashes(step3, "loc2", { Crate: H1 }), step3);
  assert.equal(ids.patchContentHashes(null, "loc1", {}), null);
});

test("fingerprints reach variants and only the named library", () => {
  const parent = { ...model("loc1", "Rock"), variants: [model("loc1", "Rock_LOD1"), model("loc1", "Rock_LOD2")] };
  const other = model("loc2", "Rock");
  const merged = { schemaVersion: 5, rigs: [], assets: [parent, other] };
  const patched = ids.patchContentHashes(merged, "loc1", { Rock: H1, Rock_LOD2: H3, constructor: H2 });
  assert.equal(patched.assets[0].contentHash, H1);
  assert.equal(patched.assets[0].variants[0].contentHash, undefined);
  assert.equal(patched.assets[0].variants[1].contentHash, H3);
  assert.equal(patched.assets[0].variants[0], parent.variants[0], "an unchanged variant keeps its identity");
  assert.equal(patched.assets[1], other, "another library is not touched");
  // Inherited object keys are not fingerprints.
  const odd = { schemaVersion: 5, rigs: [], assets: [model("loc1", "toString")] };
  assert.equal(ids.patchContentHashes(odd, "loc1", {}), odd);
  // It is plain data on the asset, so a save passes it on to a backend that does not store it.
  assert.equal(ids.splitCatalog(patched).get("loc1").assets[0].contentHash, H1);
});

test("the Libraries filter and fingerprints together", () => {
  const all = [hashed(model("loc1", "a"), H1), hashed(model("rem1", "b"), H1), hashed(model("rem1", "c"), H2)];
  assert.deepEqual(idsOf(dup.dedupeAssets(all, ranks).assets), ["loc1::a", "rem1::c"]);
  assert.deepEqual(idsOf(dup.dedupeAssets(all, ranks, new Set(["rem1"])).assets), ["rem1::b", "rem1::c"], "the selected library's own copies; same-library content is not merged");
  assert.deepEqual(dup.dedupeAssets(all, ranks, new Set(["rem1"])).alsoIn.get("rem1::b").map((x) => x.library), ["loc1"]);
});

test("20,000 fingerprinted assets, many of them identical, de-duplicate in linear time", () => {
  const all = [];
  for (let i = 0; i < 10_000; i++) {
    // loc1: ten identical kit pieces per fingerprint; the other libraries hold renamed copies of them.
    all.push(hashed(model("loc1", `a${i}`), `h${Math.floor(i / 10)}`));
    all.push(hashed(model(i % 2 ? "loc3" : "rem1", `renamed${i}`), `h${Math.floor(i / 10)}`));
  }
  const started = performance.now();
  const out = dup.dedupeAssets(all, ranks);
  const scoped = dup.dedupeAssets(all, ranks, new Set(["rem1", "loc3"]));
  const elapsed = performance.now() - started;
  assert.equal(out.assets.length, 10_000, "the kit stays whole in loc1; every renamed copy is hidden");
  assert.equal(scoped.assets.length, 5_000, "between loc3 and rem1, loc3 shows its own and hides rem1's copies");
  assert.ok(elapsed < 3_000, `took ${elapsed} ms`);
});
