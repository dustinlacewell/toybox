import { describe, expect, it } from "vitest";
import type { DraftFile, FileUse } from "./draft.js";
import {
  canImport,
  importable,
  problemsFor,
  rename,
  setChannel,
  setAllIncluded,
  setIncluded,
  setUse,
  type ReviewItem,
} from "./edit.js";

const file = (fileName: string, use: FileUse): DraftFile => ({
  relPath: `d/${fileName}`,
  fileName,
  use,
});
const item = (name: string, included = true, ...extra: DraftFile[]): ReviewItem => ({
  included,
  draft: { name, dir: "d", files: [file(`${name}_albedo.png`, { use: "albedo" }), ...extra] },
});

describe("edits", () => {
  it("setIncluded flips one item and leaves the input alone", () => {
    const items = [item("A"), item("B")];
    const next = setIncluded(items, 1, false);
    expect(next.map((i) => i.included)).toEqual([true, false]);
    expect(items[1].included).toBe(true);
  });

  it("setAllIncluded sets only the listed items and leaves the input alone", () => {
    const items = [item("A"), item("B", false), item("C")];
    const next = setAllIncluded(items, [1, 2], false);
    expect(next.map((i) => i.included)).toEqual([true, false, false]);
    expect(setAllIncluded(items, [1], true).map((i) => i.included)).toEqual([true, true, true]);
    expect(items.map((i) => i.included)).toEqual([true, false, true]);
  });

  it("rename changes one draft's name", () => {
    const next = rename([item("A"), item("B")], 0, "Z");
    expect(next.map((i) => i.draft.name)).toEqual(["Z", "B"]);
  });

  it("setUse changes one file's use", () => {
    const next = setUse([item("A")], 0, 0, { use: "emissive" });
    expect(next[0].draft.files[0].use).toEqual({ use: "emissive" });
  });

  it("setChannel sets and clears a channel on a channels file", () => {
    const items = [item("A", true, file("A_x.png", { use: "channels", channels: {} }))];
    const set = setChannel(items, 0, 1, "g", "roughness");
    expect(set[0].draft.files[1].use).toEqual({ use: "channels", channels: { g: "roughness" } });
    const cleared = setChannel(set, 0, 1, "g", undefined);
    expect(cleared[0].draft.files[1].use).toEqual({ use: "channels", channels: { g: undefined } });
  });

  it("setChannel ignores a file that is not a channels file", () => {
    const items = [item("A")];
    expect(setChannel(items, 0, 0, "r", "ao")).toEqual(items);
  });
});

describe("problemsFor", () => {
  it("passes the entry's own problems through", () => {
    const items: ReviewItem[] = [{ included: true, draft: { name: "A", dir: "d", files: [] } }];
    expect(problemsFor(items, 0, "p", "c")).toContain("no albedo map");
  });

  it("flags a name used by another included item, ignoring case", () => {
    const items = [item("Rock"), item("rock")];
    expect(problemsFor(items, 0, "p", "c")).toEqual(["name used twice"]);
    expect(problemsFor(items, 1, "p", "c")).toEqual(["name used twice"]);
  });

  it("ignores excluded items when checking for a repeated name", () => {
    const items = [item("Rock"), item("Rock", false)];
    expect(problemsFor(items, 0, "p", "c")).toEqual([]);
  });

  it("flags empty, forbidden-character, and edge space or dot names", () => {
    for (const name of ["", "a/b", "a:b", "a*b", "a?b", 'a"b', "a<b", "a|b", "a\\b", " a", "a ", ".a", "a."]) {
      expect(problemsFor([item(name)], 0, "p", "c")).toContain("invalid name");
    }
    expect(problemsFor([item("fine name")], 0, "p", "c")).toEqual([]);
  });
});

describe("importable", () => {
  it("keeps included items that have no problems", () => {
    const bad: ReviewItem = { included: true, draft: { name: "Bad", dir: "d", files: [] } };
    const items = [item("A"), item("B", false), bad, item("C")];
    expect(importable(items, "p", "c").map((i) => i.draft.name)).toEqual(["A", "C"]);
  });
});

describe("canImport", () => {
  it("needs an importable item, a pack and a category", () => {
    const items = [item("A")];
    expect(canImport(items, "p", "c")).toBe(true);
    expect(canImport(items, " ", "c")).toBe(false);
    expect(canImport(items, "p", "")).toBe(false);
    expect(canImport([item("A", false)], "p", "c")).toBe(false);
  });
});
