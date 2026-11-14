import { describe, expect, it } from "vitest";
import type { DraftFile, FileUse, MaterialDraft } from "./draft.js";
import { toEntry, usedFiles } from "./entry.js";

const file = (fileName: string, use: FileUse): DraftFile => ({
  relPath: `d/${fileName}`,
  fileName,
  use,
});
const draft = (...files: DraftFile[]): MaterialDraft => ({ name: "Cobble", dir: "d", files });
const albedo = file("Cobble_albedo.png", { use: "albedo" });
const orm = file("Cobble_orm.png", {
  use: "channels",
  channels: { r: "ao", g: "roughness", b: "metallic" },
});

describe("toEntry", () => {
  it("maps a packed ORM file to three maps sharing one file", () => {
    const res = toEntry(draft(albedo, orm), "stone_pack", "stone");
    expect(res).toEqual({
      entry: {
        kind: "material",
        id: "uid://import_stone_pack_material_Cobble",
        pack: "stone_pack",
        category: "stone",
        name: "Cobble",
        maps: [
          { role: "albedo", file: "Cobble_albedo.png" },
          { role: "ao", file: "Cobble_orm.png", channel: "r" },
          { role: "roughness", file: "Cobble_orm.png", channel: "g" },
          { role: "metallic", file: "Cobble_orm.png", channel: "b" },
        ],
        flipNormalGreen: false,
      },
    });
  });

  it("maps smoothness to an inverted roughness", () => {
    const smooth = file("Cobble_gloss.png", { use: "channels", channels: { r: "smoothness" } });
    const res = toEntry(draft(albedo, smooth), "p", "c");
    expect("entry" in res && res.entry.maps).toContainEqual({
      role: "roughness",
      file: "Cobble_gloss.png",
      channel: "r",
      invert: true,
    });
  });

  it("sets flipNormalGreen for a DirectX normal", () => {
    const nrm = file("Cobble_normal_dx.png", { use: "normal", directX: true });
    const res = toEntry(draft(albedo, nrm), "p", "c");
    expect("entry" in res && res.entry.flipNormalGreen).toBe(true);
    expect("entry" in res && res.entry.maps).toContainEqual({
      role: "normal",
      file: "Cobble_normal_dx.png",
    });
  });

  it("reports a missing albedo", () => {
    expect(toEntry(draft(orm), "p", "c")).toEqual({ problems: ["no albedo map"] });
  });

  it("reports a role assigned twice", () => {
    const rough = file("Cobble_roughness.png", {
      use: "channels",
      channels: { r: "roughness" },
    });
    expect(toEntry(draft(albedo, orm, rough), "p", "c")).toEqual({
      problems: ["roughness assigned twice"],
    });
  });

  it("counts smoothness as roughness for the twice check", () => {
    const smooth = file("Cobble_gloss.png", { use: "channels", channels: { r: "smoothness" } });
    expect(toEntry(draft(albedo, orm, smooth), "p", "c")).toEqual({
      problems: ["roughness assigned twice"],
    });
  });

  it("reports every problem at once", () => {
    const a = file("x.png", { use: "channels", channels: { r: "ao" } });
    const b = file("y.png", { use: "channels", channels: { r: "ao" } });
    expect(toEntry(draft(a, b), "p", "c")).toEqual({
      problems: ["no albedo map", "ao assigned twice"],
    });
  });

  it("reports a repeated file name", () => {
    const dup = { ...albedo, relPath: "other/Cobble_albedo.png", use: { use: "unassigned" } as FileUse };
    const res = toEntry(draft(albedo, dup), "p", "c");
    expect("problems" in res && res.problems).toEqual(["file name used twice: Cobble_albedo.png"]);
  });
});

describe("usedFiles", () => {
  it("returns only files that contribute a map", () => {
    const extra = file("notes.png", { use: "unassigned" });
    expect(usedFiles(draft(albedo, orm, extra)).map((f) => f.fileName)).toEqual([
      "Cobble_albedo.png",
      "Cobble_orm.png",
    ]);
  });
});
