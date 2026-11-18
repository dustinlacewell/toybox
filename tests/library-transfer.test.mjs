import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
const load = async (file) => {
  const { outputText } = ts.transpileModule(readFileSync(new URL(`../src/domain/${file}`, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
};
const t = await load("transfer.ts");

const lib = (id, extra = {}) => ({ id, name: id.toUpperCase(), kind: "local", root: `/${id}`, available: true, canWrite: true, isDefault: false, ...extra });
const asset = (library, raw, extra = {}) => ({ id: `${library}::${raw}`, library, name: raw, kind: "model", ...extra });
const line = (id, outcome, extra = {}) => ({ id, name: id, kind: "model", outcome, sourceRemoved: false, ...extra });

test("transfer is offered only with more than one library", () => {
  assert.equal(t.canOfferTransfer([lib("a")]), false);
  assert.equal(t.canOfferTransfer([lib("a"), lib("b")]), true);
});

test("targets are the writable libraries, without the single source library", () => {
  const libs = [lib("a"), lib("b"), lib("c", { canWrite: false }), lib("d", { available: false, canWrite: false })];
  const one = t.transferTargets(libs, ["a"]);
  assert.deepEqual(one.targets.map((l) => l.id), ["b"]);
  assert.deepEqual(one.blocked.map((b) => [b.library.id, b.reason]), [["c", "it is read-only"], ["d", "its folder cannot be read right now"]]);
  // A selection from several libraries can go into any writable one of them.
  assert.deepEqual(t.transferTargets(libs, ["a", "b"]).targets.map((l) => l.id), ["a", "b"]);
  assert.deepEqual(t.transferTargets(libs, []).targets.map((l) => l.id), ["a", "b"]);
  assert.deepEqual(t.writableLibraries(libs).map((l) => l.id), ["a", "b"]);
});

test("a move needs every source library to be writable, the target's own items excepted", () => {
  const libs = [lib("a"), lib("b", { canWrite: false }), lib("c")];
  assert.deepEqual(t.moveBlockers(libs, ["a"], "c"), []);
  assert.deepEqual(t.moveBlockers(libs, ["a", "b"], "c").map((x) => x.library.id), ["b"]);
  assert.deepEqual(t.moveBlockers(libs, ["b"], "b"), [], "nothing is removed from the target");
});

test("read-only owners of a selection are named, nothing when all can be written", () => {
  const libs = [lib("a"), lib("b", { canWrite: false })];
  assert.deepEqual(t.readOnlyOwners(libs, [asset("a", "x")]), []);
  assert.equal(t.readOnlyNotice("delete", t.readOnlyOwners(libs, [asset("a", "x")])), undefined);
  const owners = t.readOnlyOwners(libs, [asset("a", "x"), asset("b", "y"), asset("b", "z")]);
  assert.equal(owners.length, 1);
  assert.equal(t.readOnlyNotice("delete", owners), "Cannot delete: B (it is read-only).");
});

test("a selection is grouped by source library and the target's own items are set aside", () => {
  const items = [asset("a", "1"), asset("b", "2"), asset("a", "3"), asset("c", "4")];
  const plan = t.planTransfer(items, "c");
  assert.deepEqual([...plan.groups.keys()], ["a", "b"]);
  assert.deepEqual(plan.groups.get("a").map((x) => x.id), ["a::1", "a::3"]);
  assert.deepEqual(plan.alreadyThere.map((x) => x.id), ["c::4"]);
  assert.equal(t.planTransfer([asset("a", "1")], "a").groups.size, 0);
});

test("the summary line names the count, the source libraries and the target", () => {
  const libs = [lib("a", { name: "resources" }), lib("b", { name: "Second" })];
  assert.equal(t.summaryLine([asset("a", "1"), asset("a", "2")], libs, "b"), "2 items from resources → Second");
  assert.equal(t.summaryLine([asset("a", "1")], libs, "b"), "1 item from resources → Second");
  assert.equal(t.summaryLine([asset("a", "1"), asset("b", "2")], libs, "b"), "2 items from resources, Second → Second");
});

test("reports of several source libraries merge, with the items already in the target as skipped", () => {
  const merged = t.mergeTransferReports(
    [
      { library: "a", report: { items: [line("m1", "copied"), line("m2", "failed", { reason: "missing file" })] } },
      { library: "b", report: { items: [line("m1", "replaced"), line("m3", "copied", { sourceRemoved: true })] } },
    ],
    [asset("c", "m9", { name: "Nine" })],
    "Third",
  );
  assert.equal(merged.items.length, 5);
  assert.deepEqual([merged.copied, merged.replaced, merged.skipped, merged.failed, merged.removed], [2, 1, 1, 1, 1]);
  // The same raw id from two libraries stays two lines, told apart by the qualified id.
  assert.deepEqual(merged.items.filter((i) => i.id === "m1").map((i) => i.assetId), ["a::m1", "b::m1"]);
  const already = merged.items.find((i) => i.assetId === "c::m9");
  assert.equal(already.outcome, "skipped");
  assert.equal(already.id, "m9");
  assert.equal(already.reason, "Already in Third");
  assert.equal(merged.items.find((i) => i.id === "m2").reason, "missing file");
});

test("a source whose call failed reports all its items as failed with the reason", () => {
  const report = t.failedGroup([asset("a", "1"), asset("a", "2")], "folder cannot be read");
  assert.deepEqual(report.items.map((i) => [i.id, i.outcome, i.reason]), [["1", "failed", "folder cannot be read"], ["2", "failed", "folder cannot be read"]]);
});

test("progress adds finished groups to the running one", () => {
  assert.deepEqual(t.overallProgress([10, 5], 0, null), { done: 0, total: 15 });
  assert.deepEqual(t.overallProgress([10, 5], 0, { done: 4, total: 10 }), { done: 4, total: 15 });
  assert.deepEqual(t.overallProgress([10, 5], 1, { done: 2, total: 5 }), { done: 12, total: 15 });
  assert.deepEqual(t.overallProgress([10, 5], 2, null), { done: 15, total: 15 });
  assert.deepEqual(t.overallProgress([10], 0, { done: 99, total: 10 }), { done: 10, total: 10 }, "never past the total");
});

test("only the target is re-read after a copy, and the sources too after a move", () => {
  assert.deepEqual(t.affectedLibraries(["a", "b"], "c", "copy"), ["c"]);
  assert.deepEqual(t.affectedLibraries(["a", "b"], "c", "move"), ["c", "a", "b"]);
  assert.deepEqual(t.affectedLibraries(["a", "c"], "c", "move"), ["c", "a"]);
});
