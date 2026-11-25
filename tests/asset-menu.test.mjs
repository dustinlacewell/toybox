import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

function moduleUrl(file) {
  const { outputText } = ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
}
const { assetMenu, menuTarget, selectionActions, SELECTION_BAR_MIN } = await import(moduleUrl("../src/domain/assetMenu.ts"));
const { placeMenu } = await import(moduleUrl("../src/ds/placeMenu.ts"));

/** The menu as text: the heading, `---` for separators, item labels with `!reason` when disabled. */
const shown = (entries) => entries.map((entry) => entry.kind === "heading" ? `# ${entry.label}` : entry.kind === "separator" ? "---" : entry.disabled ? `${entry.label}!${entry.reason}` : entry.label);
const wellFormed = (entries) => {
  const rows = entries.slice(1);
  assert.notEqual(rows[0]?.kind, "separator");
  assert.notEqual(rows.at(-1)?.kind, "separator");
  rows.forEach((entry, index) => assert.ok(!(entry.kind === "separator" && rows[index + 1]?.kind === "separator"), "two separators in a row"));
};

test("a right-click acts on the selection when the item is in it, else on the item alone", () => {
  assert.deepEqual(menuTarget("b", new Set(["c", "b", "a"])), ["c", "b", "a"]);
  assert.deepEqual(menuTarget("x", new Set(["a", "b"])), ["x"]);
  assert.deepEqual(menuTarget("x", new Set()), ["x"]);
});

test("one local item: named heading, Open and Show in folder, no transfer and no Clear selection", () => {
  const entries = assetMenu({ target: { ids: ["a"], single: { name: "Crate", remote: false } }, selectionSize: 0, offerTransfer: false });
  wellFormed(entries);
  assert.deepEqual(shown(entries), ["# Crate", "Open", "---", "Add to collection", "Tag…", "---", "Show in folder", "Export", "---", "Delete"]);
  assert.equal(entries.at(-1).tone, "danger");
});

test("a selection of three: counted heading, transfer and Clear selection, no Open or Show in folder", () => {
  const entries = assetMenu({ target: { ids: ["a", "b", "c"] }, selectionSize: 3, offerTransfer: true });
  wellFormed(entries);
  assert.deepEqual(shown(entries), ["# 3 selected", "Add to collection", "Tag…", "Copy to library…", "Move to library…", "---", "Export", "---", "Clear selection", "Delete"]);
});

test("a remote item has no Show in folder", () => {
  const entries = assetMenu({ target: { ids: ["a"], single: { name: "Crate", remote: true } }, selectionSize: 2, offerTransfer: false });
  wellFormed(entries);
  assert.ok(!shown(entries).includes("Show in folder"));
  assert.ok(shown(entries).includes("Clear selection"));
});

test("items locked in their library cannot be tagged, moved or deleted, and say why; copying stays", () => {
  const entries = assetMenu({ target: { ids: ["a", "b"] }, selectionSize: 2, offerTransfer: true, sourceLocked: "read-only" });
  wellFormed(entries);
  assert.ok(shown(entries).includes("Delete!read-only"));
  assert.ok(shown(entries).includes("Move to library…!read-only"));
  assert.deepEqual(entries.filter((entry) => entry.disabled).map((entry) => entry.id), ["tag", "move", "delete"]);
});

test("the selection bar offers the menu's actions for several items, Export last", () => {
  const bar = selectionActions({ offerTransfer: true });
  assert.deepEqual(shown(bar), ["Add to collection", "Tag…", "Copy to library…", "Move to library…", "Delete", "Export"]);
  const menu = assetMenu({ target: { ids: ["a", "b"] }, selectionSize: 2, offerTransfer: true }).filter((entry) => entry.kind === "item" && entry.id !== "clear");
  assert.deepEqual(new Set(bar.map((entry) => entry.id)), new Set(menu.map((entry) => entry.id)));
  assert.equal(bar.find((entry) => entry.id === "delete").tone, "danger");
  assert.equal(SELECTION_BAR_MIN, 2);
});

test("the selection bar has no transfer without another library, and locks what the menu locks", () => {
  assert.deepEqual(shown(selectionActions({ offerTransfer: false })), ["Add to collection", "Tag…", "Delete", "Export"]);
  const locked = selectionActions({ offerTransfer: true, sourceLocked: "read-only" });
  assert.deepEqual(locked.filter((entry) => entry.disabled).map((entry) => entry.id), ["tag", "move", "delete"]);
  assert.ok(locked.every((entry) => !entry.disabled || entry.reason === "read-only"));
});

const viewport = { width: 1000, height: 800 };
const size = { width: 240, height: 300 };

test("a menu that fits opens at the point", () => {
  assert.deepEqual(placeMenu({ x: 100, y: 120 }, size, viewport, 8), { left: 100, top: 120 });
});

test("a menu near the right or bottom edge moves left or up and stays inside the margin", () => {
  const right = placeMenu({ x: 900, y: 100 }, size, viewport, 8);
  assert.equal(right.top, 100);
  assert.ok(right.left + size.width <= viewport.width - 8 && right.left >= 8);
  assert.equal(right.left, 660);
  const bottom = placeMenu({ x: 100, y: 700 }, size, viewport, 8);
  assert.equal(bottom.left, 100);
  assert.ok(bottom.top + size.height <= viewport.height - 8 && bottom.top >= 8);
  assert.equal(bottom.top, 400);
  const flipClamped = placeMenu({ x: 200, y: 100 }, { width: 240, height: 300 }, { width: 400, height: 800 }, 8);
  assert.equal(flipClamped.left, 8);
});

test("a menu larger than the viewport is pinned at the margin", () => {
  assert.deepEqual(placeMenu({ x: 50, y: 50 }, { width: 500, height: 900 }, { width: 400, height: 600 }, 8), { left: 8, top: 8 });
});
