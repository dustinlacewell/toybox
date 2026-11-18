import { describe, expect, it } from "vitest";
import type { FileUse } from "./draft.js";
import { detectUse } from "./detect.js";

const ch = (channels: Record<string, string>): FileUse =>
  ({ use: "channels", channels }) as FileUse;

describe("detectUse suffix families", () => {
  const cases: [string, string, FileUse][] = [
    ["Stone_albedo.png", "Stone", { use: "albedo" }],
    ["Stone_diffuse.png", "Stone", { use: "albedo" }],
    ["Stone_nrm.png", "Stone", { use: "normal", directX: false }],
    ["Stone_normalgl.png", "Stone", { use: "normal", directX: false }],
    ["Stone_normaldx.png", "Stone", { use: "normal", directX: true }],
    ["Stone_emission.png", "Stone", { use: "emissive" }],
    ["Stone_displacement.png", "Stone", { use: "height" }],
    ["Stone_rough.png", "Stone", ch({ r: "roughness" })],
    ["Stone_gloss.png", "Stone", ch({ r: "smoothness" })],
    ["Stone_metalness.png", "Stone", ch({ r: "metallic" })],
    ["Stone_occlusion.png", "Stone", ch({ r: "ao" })],
    ["Stone_alpha.png", "Stone", ch({ r: "opacity" })],
    ["Stone_orm.png", "Stone", ch({ r: "ao", g: "roughness", b: "metallic" })],
    ["Stone_arm.png", "Stone", ch({ r: "ao", g: "roughness", b: "metallic" })],
    ["Stone_rma.png", "Stone", ch({ r: "roughness", g: "metallic", b: "ao" })],
    ["Stone_maskmap.png", "Stone", ch({ r: "metallic", g: "ao", a: "smoothness" })],
  ];
  it.each(cases)("%s", (file, base, use) => {
    expect(detectUse(file)).toEqual({ base, use });
  });
});

describe("detectUse naming", () => {
  it("strips trailing resolution tokens", () => {
    expect(detectUse("Wood_Albedo_4K.png")).toEqual({ base: "Wood", use: { use: "albedo" } });
    expect(detectUse("Wood_Albedo_2048_4k.png").base).toBe("Wood");
  });

  it("reads two-token suffixes", () => {
    expect(detectUse("Rock_Base_Color.jpg")).toEqual({ base: "Rock", use: { use: "albedo" } });
    expect(detectUse("Rock_Normal_DX.png")).toEqual({
      base: "Rock",
      use: { use: "normal", directX: true },
    });
    expect(detectUse("Rock_Normal_GL.png").use).toEqual({ use: "normal", directX: false });
    expect(detectUse("Rock_Ambient_Occlusion.png").use).toEqual(ch({ r: "ao" }));
    expect(detectUse("Rock_Mask_Map.png").use).toEqual(
      ch({ r: "metallic", g: "ao", a: "smoothness" }),
    );
  });

  it("is case-insensitive and keeps the base's own case and separators", () => {
    expect(detectUse("Mossy-Cobble 01 ALBEDO.PNG")).toEqual({
      base: "Mossy-Cobble 01",
      use: { use: "albedo" },
    });
  });

  it("leaves an unmatched file unassigned with the resolution removed", () => {
    expect(detectUse("Wood_Thing_4k.png")).toEqual({
      base: "Wood_Thing",
      use: { use: "unassigned" },
    });
    expect(detectUse("Notes.png")).toEqual({ base: "Notes", use: { use: "unassigned" } });
  });

  it("gives an empty base for a bare suffix name", () => {
    expect(detectUse("albedo.png")).toEqual({ base: "", use: { use: "albedo" } });
  });
});
