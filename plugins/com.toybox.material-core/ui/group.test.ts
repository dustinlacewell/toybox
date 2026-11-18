import { describe, expect, it } from "vitest";
import { groupMaterials } from "./group.js";

const summary = (paths: string[]) =>
  groupMaterials(paths).map((d) => ({
    name: d.name,
    dir: d.dir,
    files: d.files.map((f) => `${f.fileName}:${f.use.use}`),
  }));

describe("groupMaterials", () => {
  it("ignores non-image files", () => {
    expect(groupMaterials(["a/readme.txt", "a/model.glb"])).toEqual([]);
  });

  it("splits a folder with two sets", () => {
    expect(
      summary([
        "tex/Brick_albedo.png",
        "tex/Brick_normal.png",
        "tex/Wood_albedo.jpg",
        "tex/Wood_roughness.jpg",
      ]),
    ).toEqual([
      { name: "Brick", dir: "tex", files: ["Brick_albedo.png:albedo", "Brick_normal.png:normal"] },
      { name: "Wood", dir: "tex", files: ["Wood_albedo.jpg:albedo", "Wood_roughness.jpg:channels"] },
    ]);
  });

  it("matches group names case-insensitively, keeping the first spelling", () => {
    const out = summary(["Brick_albedo.png", "brick_normal.png"]);
    expect(out).toHaveLength(1);
    expect(out[0].name).toBe("Brick");
  });

  it("joins a bare-stem file as albedo", () => {
    expect(summary(["d/Wood.png", "d/Wood_normal.png", "d/Brick_normal.png"])).toEqual([
      { name: "Brick", dir: "d", files: ["Brick_normal.png:normal"] },
      { name: "Wood", dir: "d", files: ["Wood.png:albedo", "Wood_normal.png:normal"] },
    ]);
  });

  it("joins a bare-stem file as unassigned when the group has an albedo", () => {
    const out = summary(["d/Wood.png", "d/Wood_albedo.png"]);
    expect(out).toEqual([
      { name: "Wood", dir: "d", files: ["Wood.png:unassigned", "Wood_albedo.png:albedo"] },
    ]);
  });

  it("attaches a stray file to the only group in its folder", () => {
    expect(summary(["d/Wood_albedo.png", "d/Wood_normal.png", "d/extra.png"])).toEqual([
      {
        name: "Wood",
        dir: "d",
        files: ["Wood_albedo.png:albedo", "Wood_normal.png:normal", "extra.png:unassigned"],
      },
    ]);
  });

  it("makes a single-image material from a stray file when several groups exist", () => {
    expect(
      summary(["d/A_albedo.png", "d/B_albedo.png", "d/Moss.jpg", "e/Lone.png"]),
    ).toEqual([
      { name: "A", dir: "d", files: ["A_albedo.png:albedo"] },
      { name: "B", dir: "d", files: ["B_albedo.png:albedo"] },
      { name: "Moss", dir: "d", files: ["Moss.jpg:albedo"] },
      { name: "Lone", dir: "e", files: ["Lone.png:albedo"] },
    ]);
  });

  it("names an empty-base group after its folder, or material at the root", () => {
    expect(summary(["stone/Cobble/albedo.png", "stone/Cobble/normal.png"])).toEqual([
      {
        name: "Cobble",
        dir: "stone/Cobble",
        files: ["albedo.png:albedo", "normal.png:normal"],
      },
    ]);
    expect(summary(["albedo.png"])[0].name).toBe("material");
  });

  it("is deterministic regardless of input order", () => {
    const paths = ["b/X_albedo.png", "a/Y_normal.png", "a/Y_albedo.png", "a/Z.png"];
    expect(groupMaterials([...paths].reverse())).toEqual(groupMaterials(paths));
  });
});
