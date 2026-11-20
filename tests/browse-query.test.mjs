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
const { ALL_TYPES, applyFilter, emptyFilter, facetState, withFacet, propertyValue } = await import(facetsUrl);
const { parseQuery, applyQuery, filterChips, quoteValue } = await import(moduleUrl("../src/domain/query.ts", { "./facets": facetsUrl }));
const { toBrowseFilter, NO_RULES } = await import(moduleUrl("../src/domain/browseQuery.ts"));

const asset = (id, extra = {}) => ({
  id, name: id, kind: "audio", category: "sfx", library: "lib", tags: [], fileName: "x.wav", relPath: "", pack: "p",
  thumb: { state: "missing" }, user: { favorite: false, tags: [], provenance: { source: "Pack A", publisher: "Northwind", license: "CC0" } },
  audio: { file: "x.wav", format: "wav", duration: 1, sampleRate: 48000, channels: 2 }, ...extra,
});

test("typed field terms become conditions, the rest stays search text", () => {
  const parsed = parseQuery(` door  tag:sci-fi -publisher:"Kitbash Works" licence:cc0 is:favorite foo:bar "two words" `);
  assert.deepEqual(parsed.terms, [
    { field: "tags", value: "sci-fi", exclude: false },
    { field: "publishers", value: "Kitbash Works", exclude: true },
    { field: "licenses", value: "cc0", exclude: false },
  ]);
  assert.equal(parsed.favorites, true);
  assert.equal(parsed.text, "door foo:bar two words");
  // A lone dash and an empty value are text.
  assert.deepEqual(parseQuery("- tag: -").terms, []);
  assert.equal(quoteValue("Kitbash Works"), '"Kitbash Works"');
  assert.equal(quoteValue("cc0"), "cc0");
});

test("a value is chosen or left out, never both", () => {
  let f = emptyFilter("audio");
  f = applyQuery(f, parseQuery("tag:loud -tag:quiet pub:Northwind boom"));
  assert.equal(f.text, "boom");
  assert.equal(facetState(f, "tags", "loud"), "in");
  assert.equal(facetState(f, "tags", "quiet"), "out");
  f = withFacet(f, "tags", "quiet", "in");
  assert.equal(facetState(f, "tags", "quiet"), "in");
  assert.equal(f.exclude.tags.size, 0);
  f = withFacet(f, "publishers", "Lowpoly", "in", true);
  assert.deepEqual([...f.publishers], ["Lowpoly"]);
  f = withFacet(f, "tags", "loud", "off");
  assert.deepEqual([...f.tags], ["quiet"]);
  assert.deepEqual(filterChips(f).map((chip) => `${chip.exclude ? "-" : ""}${chip.label}:${chip.text}`), ["publisher:Lowpoly", "tag:quiet"]);
});

test("the window's own filter leaves out, matches any tag, reads ranges and shows every type", () => {
  const assets = [
    asset("a", { tags: ["loud"] }),
    asset("b", { tags: ["quiet"], audio: { duration: 4 } }),
    asset("c", { category: "laser", user: { favorite: false, tags: ["mine"], provenance: { source: "Pack B", publisher: "Kitbash", license: "MIT" } } }),
    { ...asset("s"), kind: "plugin", type: "skybox", audio: undefined, meta: { width: 2048 } },
  ];
  const ids = (f) => applyFilter(assets, f).map((a) => a.id);
  let f = emptyFilter("audio");
  assert.deepEqual(ids(f), ["a", "b", "c"]);
  assert.deepEqual(ids(withFacet(f, "publishers", "Kitbash", "out")), ["a", "b"]);
  assert.deepEqual(ids(withFacet(f, "tags", "loud", "out")), ["b", "c"]);
  assert.deepEqual(ids(withFacet(f, "categories", "laser", "out")), ["a", "b"]);
  const two = withFacet(withFacet(f, "tags", "loud", "in"), "tags", "mine", "in");
  assert.deepEqual(ids(two), []);
  assert.deepEqual(ids({ ...two, tagsMatch: "any" }), ["a", "c"]);
  assert.deepEqual(ids({ ...f, ranges: { "audio.duration": { min: 2 } } }), ["b"]);
  f = emptyFilter(ALL_TYPES);
  assert.deepEqual(ids(f), ["a", "b", "c", "s"]);
  assert.deepEqual(ids({ ...f, ranges: { "meta.width": { min: 1024, max: 4096 } } }), ["s"]);
  assert.equal(propertyValue(assets[3], "meta.width"), 2048);
  assert.equal(propertyValue(assets[3], "meta.nope.deeper"), undefined);
});

test("the question the backend gets", () => {
  let f = applyQuery(emptyFilter(ALL_TYPES), parseQuery("tag:b tag:a -source:x -tag:z"));
  f = { ...f, tagsMatch: "any", ranges: { "meta.width": { min: 10 }, "audio.duration": { max: 3 } }, sort: { key: "prop:audio.duration", descending: true } };
  const q = toBrowseFilter(f, null, NO_RULES);
  assert.equal(q.kind, "*");
  assert.deepEqual(q.tags, ["a", "b"]);
  assert.equal(q.tagsAny, true);
  assert.deepEqual(q.exclude, { sources: ["x"], publishers: [], licenses: [], formats: [], categories: [], tags: ["z"] });
  assert.deepEqual(q.ranges, [{ path: "audio.duration", min: null, max: 3 }, { path: "meta.width", min: 10, max: null }]);
  assert.deepEqual(q.sort, { key: "prop:audio.duration", descending: true });
  // "Any" means nothing with one tag.
  assert.equal(toBrowseFilter({ ...emptyFilter("audio"), tags: new Set(["a"]), tagsMatch: "any" }, null, NO_RULES).tagsAny, false);
});

test("a saved view keeps the whole question", async () => {
  const { storeFilter, readFilter } = await import(moduleUrl("../src/domain/savedViews.ts", { "./facets": facetsUrl }));
  let f = applyQuery(emptyFilter("audio"), parseQuery("boom tag:a -publisher:x is:fav"));
  f = { ...f, tagsMatch: "any", ranges: { "audio.duration": { min: 1 } }, sort: { key: "added", descending: true } };
  const back = readFilter(JSON.parse(JSON.stringify(storeFilter(f))));
  assert.deepEqual(storeFilter(back), storeFilter(f));
  // Junk from an older version reads as an empty filter of a type.
  const junk = readFilter({ kind: 3, tags: "x", exclude: null, ranges: { p: { min: "1" } }, sort: 1 });
  assert.equal(junk.kind, "model");
  assert.equal(junk.tags.size, 0);
  assert.deepEqual(junk.ranges, {});
  assert.deepEqual(junk.sort, { key: "name", descending: false });
});

test("a long field is searchable and shortened, its chosen values always shown", async () => {
  const { narrowRows } = await import(facetsUrl);
  const rows = ["Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta"].map((value) => ({ value, count: 1 }));
  const label = (row) => row.value;
  const kept = new Set(["Zeta"]);
  const keep = (value) => kept.has(value);
  assert.deepEqual(narrowRows(rows, "ta", keep, label, 10, false).rows.map(label), ["Beta", "Delta", "Zeta"]);
  assert.deepEqual(narrowRows(rows, "ALP", keep, label, 10, false).rows.map(label), ["Alpha", "Zeta"]);
  const short = narrowRows(rows, "", keep, label, 2, false);
  assert.deepEqual(short.rows.map(label), ["Alpha", "Beta", "Zeta"]);
  assert.equal(short.hidden, 3);
  assert.equal(narrowRows(rows, "", keep, label, 2, true).rows.length, 6);
});
