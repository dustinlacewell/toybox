import { describe, expect, it } from "vitest";
import type { DraftFile, FileUse, MaterialDraft } from "./draft.js";
import type { ReviewItem } from "./edit.js";
import { attentionIndexes, previewFile, roleLabels } from "./summary.js";

const file = (fileName: string, use: FileUse): DraftFile => ({
  relPath: `d/${fileName}`,
  fileName,
  use,
});
const draft = (name: string, ...files: DraftFile[]): MaterialDraft => ({ name, dir: "d", files });

describe("previewFile", () => {
  it("prefers the albedo file", () => {
    const d = draft("A", file("n.png", { use: "normal", directX: false }), file("a.png", { use: "albedo" }));
    expect(previewFile(d)?.fileName).toBe("a.png");
  });

  it("without an albedo, takes the first file that is used", () => {
    const d = draft("A", file("x.png", { use: "unassigned" }), file("h.png", { use: "height" }));
    expect(previewFile(d)?.fileName).toBe("h.png");
  });

  it("with nothing used, takes the first file", () => {
    const d = draft("A", file("x.png", { use: "unassigned" }), file("y.png", { use: "unassigned" }));
    expect(previewFile(d)?.fileName).toBe("x.png");
  });

  it("is undefined for no files", () => {
    expect(previewFile(draft("A"))).toBeUndefined();
  });
});

describe("roleLabels", () => {
  it("lists single-role files in the fixed order", () => {
    const d = draft(
      "A",
      file("e.png", { use: "emissive" }),
      file("n.png", { use: "normal", directX: true }),
      file("a.png", { use: "albedo" }),
    );
    expect(roleLabels(d)).toEqual(["Albedo", "Normal", "Emissive"]);
  });

  it("includes each assigned channel of a packed file", () => {
    const d = draft(
      "A",
      file("a.png", { use: "albedo" }),
      file("orm.png", { use: "channels", channels: { r: "ao", g: "roughness", b: "metallic" } }),
    );
    expect(roleLabels(d)).toEqual(["Albedo", "Roughness", "Metallic", "AO"]);
  });

  it("counts a smoothness channel as roughness and drops duplicates", () => {
    const d = draft(
      "A",
      file("r.png", { use: "channels", channels: { r: "roughness" } }),
      file("s.png", { use: "channels", channels: { a: "smoothness" } }),
    );
    expect(roleLabels(d)).toEqual(["Roughness"]);
  });

  it("handles a draft with no albedo and skips unassigned files", () => {
    const d = draft(
      "A",
      file("u.png", { use: "unassigned" }),
      file("o.png", { use: "channels", channels: { g: "opacity" } }),
    );
    expect(roleLabels(d)).toEqual(["Opacity"]);
  });
});

describe("attentionIndexes", () => {
  const good = (name: string, included = true): ReviewItem => ({
    included,
    draft: draft(name, file(`${name}.png`, { use: "albedo" })),
  });

  it("lists included items with problems", () => {
    const noAlbedo: ReviewItem = { included: true, draft: draft("B") };
    expect(attentionIndexes([good("A"), noAlbedo, good("C")], "p", "c")).toEqual([1]);
  });

  it("skips excluded items even when they have problems", () => {
    const noAlbedo: ReviewItem = { included: false, draft: draft("B") };
    expect(attentionIndexes([good("A"), noAlbedo], "p", "c")).toEqual([]);
  });

  it("flags both items of a name used twice", () => {
    expect(attentionIndexes([good("A"), good("a")], "p", "c")).toEqual([0, 1]);
  });
});
