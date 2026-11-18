import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
const load = async (file) => {
  const { outputText } = ts.transpileModule(readFileSync(new URL(`../src/domain/${file}`, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
};
const { PageCache, visibleRows } = await load("pageCache.ts");
const query = await load("browseQuery.ts");

const rows = (page, size = 100) => Array.from({ length: size }, (_, i) => `row ${page * size + i}`);
const answer = (cache, page, total = 1000) => cache.accept(cache.begin(page), rows(page), total);

test("the first screen asks for the page it shows, and only that until the total is known", () => {
  const cache = new PageCache();
  assert.deepEqual(cache.need(0, 29), [0]);
  const ticket = cache.begin(0);
  assert.deepEqual(cache.need(0, 29), [], "no second request for a page that is on its way");
  assert.ok(cache.accept(ticket, rows(0), 1000));
  assert.equal(cache.rowAt(5), "row 5");
  assert.equal(cache.rowAt(100), undefined);
  assert.deepEqual(cache.need(0, 29), [1], "the page beyond the screen is fetched ahead");
});

test("a jump to the middle of a million rows asks for the pages there, not the ones passed", () => {
  const cache = new PageCache();
  answer(cache, 0, 1_000_000);
  // Scrolling fast through 10,000 pages: each step asks again, but at most two requests are ever outstanding.
  let outstanding = 0, started = 0;
  for (let page = 1; page < 9000; page += 7) {
    for (const wanted of cache.need(page * 100, page * 100 + 29)) { cache.begin(wanted); outstanding++; started++; }
    assert.ok(cache.inflightPages().length <= 2, "never more than two in flight");
  }
  assert.ok(started <= 2, `requests started while all slots were taken: ${started}`);
  assert.ok(outstanding <= 2);
});

test("free slots go to the pages on screen, nearest the middle first", () => {
  const cache = new PageCache();
  answer(cache, 0, 100_000);
  const wanted = cache.need(50_000, 50_250); // pages 500..502 on screen
  assert.deepEqual(wanted, [501, 500]);
  for (const page of wanted) cache.begin(page);
  assert.deepEqual(cache.need(50_000, 50_250), []);
});

test("an answer to an earlier question is ignored", () => {
  const cache = new PageCache();
  const old = cache.begin(0);
  cache.reset();
  assert.equal(cache.accept(old, rows(0), 5), false);
  assert.equal(cache.rowAt(0), undefined);
  assert.equal(cache.total, null);
  const fresh = cache.begin(0);
  assert.ok(cache.accept(fresh, rows(0, 3), 3));
  assert.equal(cache.total, 3);
  // A failure of an old request does not free the slot of the new one.
  const another = cache.begin(1);
  cache.fail(old);
  assert.deepEqual(cache.inflightPages(), [1]);
  cache.fail(another);
  assert.deepEqual(cache.inflightPages(), []);
});

test("a refresh after a write keeps the rows on screen until the new page arrives", () => {
  const cache = new PageCache();
  answer(cache, 0);
  cache.reset(true);
  assert.equal(cache.rowAt(3), "row 3", "still shown");
  assert.ok(cache.isStale(3));
  assert.equal(cache.need(0, 29)[0], 0, "and asked for again, the page on screen first");
  cache.accept(cache.begin(0), rows(0).map((row) => `${row}!`), 999);
  assert.equal(cache.rowAt(3), "row 3!");
  assert.ok(!cache.isStale(3));
  assert.equal(cache.total, 999);
});

test("pages far from the screen are dropped first, never the ones in view", () => {
  const cache = new PageCache(100, 4, 2);
  for (const page of [0, 1, 2, 3]) answer(cache, page);
  cache.need(0, 29); // page 0 is in view
  answer(cache, 8);
  assert.equal(cache.loadedPages(), 4);
  assert.ok(cache.has(0), "the page in view stays");
  assert.ok(cache.has(8), "the newest stays");
  assert.ok(!cache.has(1), "the least recently used goes");
});

test("rows are patched in place without losing the page", () => {
  const cache = new PageCache();
  answer(cache, 0);
  assert.ok(cache.patch((row) => (row === "row 2" ? "row two" : row)));
  assert.equal(cache.rowAt(2), "row two");
  assert.equal(cache.rowAt(3), "row 3");
  assert.equal(cache.patch((row) => row), false);
});

test("an empty list asks for nothing", () => {
  const cache = new PageCache();
  cache.accept(cache.begin(0), [], 0);
  assert.deepEqual(cache.need(0, 20), []);
});

test("the rows on screen follow the scroll position", () => {
  assert.deepEqual(visibleRows(0, 600, 200, 5, 1000), { first: 0, last: 19 });
  assert.deepEqual(visibleRows(100_000, 600, 200, 5, 1_000_000), { first: 2500, last: 2519 });
  assert.deepEqual(visibleRows(0, 600, 200, 5, 0), { first: 0, last: -1 });
  assert.equal(visibleRows(1e9, 600, 200, 5, 100).last, 99);
});

test("the filter is sent as sorted lists, and equal filters have equal signatures", () => {
  const filter = { kind: "model", libraries: new Set(["b", "a"]), sources: new Set(), publishers: new Set(), licenses: new Set(), formats: new Set(), categories: new Set(["x"]), tags: new Set(["t2", "t1"]), favoritesOnly: false, text: "  rock " };
  const a = query.toBrowseFilter(filter, "Zoo", query.NO_RULES);
  assert.deepEqual(a.libraries, ["a", "b"]);
  assert.deepEqual(a.tags, ["t1", "t2"]);
  assert.equal(a.text, "rock");
  assert.equal(a.collection, "Zoo");
  const b = query.toBrowseFilter({ ...filter, libraries: new Set(["a", "b"]) }, "Zoo", query.NO_RULES);
  assert.equal(query.filterSignature(a), query.filterSignature(b));
  assert.notEqual(query.filterSignature(a), query.filterSignature(query.toBrowseFilter({ ...filter, text: "rocks" }, "Zoo", query.NO_RULES)));
});

test("an exact length that arrives apart from a page replaces the lower bound, and only for the question it was asked for", () => {
  const cache = new PageCache();
  const first = cache.begin(0);
  // A broad search answers with what the pages prove: this page and one more row.
  assert.ok(cache.accept(first, rows(0), 101));
  assert.equal(cache.total, 101);
  assert.deepEqual(cache.need(0, 29), [1], "the row beyond the page is asked for, as the lower bound allows");
  const generation = cache.generation;
  assert.ok(cache.setTotal(generation, 23337));
  assert.equal(cache.total, 23337);
  // The scrollbar can now reach the end: pages far down are asked for.
  assert.equal(cache.need(23000, 23029)[0], 230);
  // A count for a question that has since changed is dropped.
  cache.reset(false);
  assert.equal(cache.setTotal(generation, 5), false);
  assert.equal(cache.total, null);
});
