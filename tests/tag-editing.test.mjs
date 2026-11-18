import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

const compile = (file) => ts.transpileModule(readFileSync(new URL(`../src/domain/${file}`, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } }).outputText;
const toUrl = (code) => `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`;
const libraryTagsUrl = toUrl(compile("libraryTags.ts"));
// tagEditing imports ./libraryTags: point that at the compiled copy.
const edit = await import(toUrl(compile("tagEditing.ts").replace(/from "\.\/libraryTags"/, `from "${libraryTagsUrl}"`)));

const user = (tags = []) => ({ favorite: false, tags });
const copy = (library, id, tags, extra = {}) => ({ kind: "model", library, id: `${library}::${id}`, name: id, fileName: `${id}.gltf`, relPath: `${id}.gltf`, pack: "p", category: "props", user: user(), thumb: { state: "missing" }, ...(tags ? { tags } : {}), ...extra });
const names = { a: "resources", b: "toybox.fornara.io", c: "archive" };
const nameOf = (id) => names[id] ?? id;
const everyoneWritable = () => null;
const reasons = (table) => (library) => table[library] ?? null;
const ids = (plan, library) => (plan.edits.get(library) ?? []).map((e) => [e.id, e.tags]);

test("the tags of an asset are the union of its copies, the shown copy first", () => {
  const shown = copy("a", "crate", ["wood", "box"]);
  const other = copy("b", "crate", ["box", "Prop", "wood"]);
  assert.deepEqual(edit.unionTags([shown, other]), ["wood", "box", "prop"]);
  assert.deepEqual(edit.unionTags([copy("a", "x")]), []);
  const lookup = (id) => (id === other.id ? other : undefined);
  assert.deepEqual(edit.copiesOf(shown, [{ id: other.id }, { id: "c::gone" }], lookup), [shown, other], "a copy the catalog does not hold is left out");
  assert.deepEqual(edit.copiesOf(shown, [], lookup), [shown]);
});

test("tags only this computer holds are listed apart, and provenance tags are not tags", () => {
  const a = copy("a", "m", ["wood"], { user: user(["source: Synty", "mine", "wood"]) });
  const b = copy("b", "m", [], { user: user(["license: CC0", "also mine"]) });
  assert.deepEqual(edit.leftoverTags([a, b], edit.unionTags([a, b])), ["mine", "also mine"]);
});

test("adding a tag to a duplicated asset writes the full union to every copy, one list per library", () => {
  const shown = copy("a", "crate", ["wood"]);
  const second = copy("b", "crate", ["metal"]);
  const third = copy("c", "crate", undefined);
  const plan = edit.planTagChange([[shown, second, third]], { add: ["  New "] }, everyoneWritable);
  assert.deepEqual(ids(plan, "a"), [["a::crate", ["wood", "metal", "new"]]]);
  assert.deepEqual(ids(plan, "b"), [["b::crate", ["wood", "metal", "new"]]], "the drifted copy converges");
  assert.deepEqual(ids(plan, "c"), [["c::crate", ["wood", "metal", "new"]]]);
  assert.equal(plan.blocked.size, 0);
  assert.equal(plan.changed, 1);
});

test("removing a tag removes it from every copy that has it", () => {
  const shown = copy("a", "crate", ["wood", "box"]);
  const second = copy("b", "crate", ["box", "metal"]);
  const plan = edit.planTagChange([[shown, second]], { remove: ["box"] }, everyoneWritable);
  assert.deepEqual(ids(plan, "a"), [["a::crate", ["wood", "metal"]]]);
  assert.deepEqual(ids(plan, "b"), [["b::crate", ["wood", "metal"]]]);
});

test("a copy that already holds the result is not written, and a no-op change plans nothing", () => {
  const shown = copy("a", "crate", ["wood", "box"]);
  const same = copy("b", "crate", ["box", "wood"]);
  assert.ok(edit.planIsEmpty(edit.planTagChange([[shown, same]], { add: ["wood"] }, everyoneWritable)), "same tags in another order is not drift");
  assert.ok(edit.planIsEmpty(edit.planTagChange([[shown]], { remove: ["missing"] }, everyoneWritable)));
  const partial = edit.planTagChange([[shown, copy("b", "crate", ["wood"])]], { add: ["box"] }, everyoneWritable);
  assert.deepEqual([...partial.edits.keys()], ["b"], "only the copy that differs");
});

test("copies in libraries that cannot be written are reported, the others are still edited", () => {
  const shown = copy("a", "crate", ["wood"]);
  const remote = copy("b", "crate", ["wood"]);
  const plan = edit.planTagChange([[shown, remote]], { add: ["new"] }, reasons({ b: "the server cannot be reached right now" }));
  assert.deepEqual([...plan.edits.keys()], ["a"]);
  assert.deepEqual([...plan.blocked], [["b", "the server cannot be reached right now"]]);
  assert.equal(edit.anyWritable([shown, remote], reasons({ b: "x" })), true);
  assert.equal(edit.anyWritable([shown, remote], reasons({ a: "it is read-only", b: "x" })), false, "no copy can be written: the editor is disabled");
  // A blocked copy that needs no change is nothing to report.
  const fine = edit.planTagChange([[copy("a", "c", ["new"]), copy("b", "c", ["new"])]], { add: ["new"] }, reasons({ b: "x" }));
  assert.ok(edit.planIsEmpty(fine));
});

test("what was saved and what was not is told per library in plain words", () => {
  const blocked = new Map([["c", "it is read-only"]]);
  const done = edit.describeResult([{ library: "a", saved: 3 }, { library: "b", saved: 0, error: "The server cannot be reached right now." }], blocked, nameOf);
  assert.equal(done.message, "Saved in resources. Not saved in toybox.fornara.io: The server cannot be reached right now. Not saved in archive: it is read-only.");
  assert.equal(done.failed, true);
  const only = edit.describeResult([{ library: "a", saved: 1 }], new Map([["b", "the server cannot be reached right now"]]), nameOf);
  assert.equal(only.message, "Saved in resources. Not saved in toybox.fornara.io: the server cannot be reached right now.");
  const clean = edit.describeResult([{ library: "a", saved: 1 }, { library: "b", saved: 1 }], new Map(), nameOf);
  assert.deepEqual(clean, { message: "Saved in resources, toybox.fornara.io.", failed: false });
  assert.equal(edit.describeResult([], new Map(), nameOf).message, "Nothing to change.");
  const lost = edit.describeResult([{ library: "a", saved: 0, error: "Boom" }], new Map(), nameOf);
  assert.equal(lost.message, "Not saved in resources: Boom.");
});

test("a selection is edited with one list of edits per library and chunked to what the backend takes", () => {
  const groups = Array.from({ length: 5003 }, (_, n) => [copy("a", `m${n}`, ["x"])]);
  const plan = edit.planTagChange(groups, { add: ["y"] }, everyoneWritable);
  assert.equal(plan.changed, 5003);
  const parts = edit.chunk(plan.edits.get("a"));
  assert.deepEqual(parts.map((part) => part.length), [5000, 3]);
  assert.deepEqual(edit.chunk([]), []);
  assert.deepEqual(edit.chunk([1, 2, 3], 2), [[1, 2], [3]]);
});

test("multi-select add and remove are planned for every selected asset and its copies", () => {
  const one = [copy("a", "one", ["wood", "shared"]), copy("b", "one", ["wood"])];
  const two = [copy("a", "two", ["shared"])];
  const three = [copy("c", "three", [])];
  const groups = [one, two, three];
  const counts = edit.selectionTagCounts(groups);
  assert.deepEqual(counts, [{ tag: "shared", count: 2 }, { tag: "wood", count: 1 }], "an asset counts once however many copies it has");

  const adding = edit.planTagChange(groups, { add: ["new", "wood"] }, reasons({ c: "it is read-only" }));
  assert.deepEqual(ids(adding, "a"), [["a::one", ["wood", "shared", "new"]], ["a::two", ["shared", "new", "wood"]]]);
  assert.deepEqual(ids(adding, "b"), [["b::one", ["wood", "shared", "new"]]]);
  assert.equal(adding.edits.has("c"), false);
  assert.equal(adding.blocked.get("c"), "it is read-only");
  assert.equal(adding.changed, 2);

  const removing = edit.planTagChange(groups, { remove: ["shared"] }, everyoneWritable);
  assert.deepEqual(ids(removing, "a"), [["a::one", ["wood"]], ["a::two", []]]);
  assert.deepEqual(ids(removing, "b"), [], "that copy had no such tag");
  assert.equal(removing.edits.has("c"), false, "nothing to remove there");
});

test("typed tags are split on commas and a colon is refused", () => {
  assert.deepEqual(edit.parseTagInput(" Foot Step, loop ,, LOOP "), { tags: ["foot step", "loop"], problem: null });
  assert.deepEqual(edit.parseTagInput(""), { tags: [], problem: null });
  const bad = edit.parseTagInput("ok, source: x");
  assert.deepEqual(bad.tags, ["ok"]);
  assert.match(bad.problem, /colon/);
});

test("personal tags that can be library tags are planned for the libraries that can be written", () => {
  const plain = copy("a", "p", ["wood"], { user: user(["source: Synty", "Mine", "wood", "odd: one"]) });
  const locked = copy("b", "q", [], { user: user(["mine"]) });
  const nothing = copy("a", "r", ["x"], { user: user(["source: Y"]) });
  const already = copy("a", "s", ["mine"], { user: user(["mine"]) });
  const plan = edit.planPersonalMigration([plain, locked, nothing, already], (library) => library === "a");
  assert.deepEqual(ids(plan, "a"), [["a::p", ["wood", "mine"]]]);
  assert.equal(plan.edits.has("b"), false, "a library that cannot be written keeps its tags where they are");
  assert.deepEqual([...plan.taken], [["a::p", ["Mine", "wood"]], ["a::s", ["mine"]]], "a tag the library already has is only taken out of the personal ones");
  assert.deepEqual(edit.planPersonalMigration([plain], () => true).taken.get("a::p"), ["Mine", "wood"], "provenance and colon tags never move");
  // Full item: nothing fits, so nothing moves.
  const full = copy("a", "f", Array.from({ length: 64 }, (_, n) => `t${n}`), { user: user(["extra"]) });
  assert.equal(edit.planPersonalMigration([full], () => true).taken.size, 0);
});
