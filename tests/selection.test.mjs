import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
const { outputText } = ts.transpileModule(readFileSync(new URL("../src/domain/selection.ts", import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
const { selectItems } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);

test("Shift selects the inclusive range in either direction without moving the anchor", () => {
  const order = ["a", "b", "c", "d", "e"];
  let result = selectItems(order, new Set(["a"]), "a", "c", "toggle");
  assert.deepEqual(result, { ids: ["a", "c"], anchor: "c" });
  result = selectItems(order, new Set(result.ids), result.anchor, "e", "range");
  assert.deepEqual(result, { ids: ["a", "c", "d", "e"], anchor: "c" });
  result = selectItems(order, new Set(result.ids), result.anchor, "b", "range");
  assert.deepEqual(new Set(result.ids), new Set(order));
  assert.equal(result.anchor, "c");
});
test("individual toggles preserve other items, while an ordinary click replaces the selection", () => {
  const order = ["a", "b", "c"];
  let result = selectItems(order, new Set(["a", "b"]), "b", "a", "toggle");
  assert.deepEqual(result, { ids: ["b"], anchor: "a" });
  result = selectItems(order, new Set(result.ids), result.anchor, "c", "replace");
  assert.deepEqual(result, { ids: ["c"], anchor: "c" });
});
test("ranges include virtual rows and follow only the visible filtered order", () => {
  const order = Array.from({ length: 100 }, (_, i) => String(i));
  const result = selectItems(order, new Set(["5"]), "5", "85", "range");
  assert.equal(result.ids.length, 81); assert.ok(result.ids.includes("50"));
  assert.deepEqual(selectItems(["a", "c", "e"], new Set(["a"]), "a", "e", "range").ids, ["a", "c", "e"]);
  assert.deepEqual(selectItems(["c", "e"], new Set(), "a", "e", "range"), { ids: ["e"], anchor: "e" });
});
