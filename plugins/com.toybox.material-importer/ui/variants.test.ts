import { describe, it, expect } from "vitest";
import { groupMaterials } from "./group.js";
import { materialGroups } from "./variants.js";
import { toEntry } from "./entry.js";
import { groupSeedVariants } from "@ldlework/toybox-sdk";

describe("size variants", () => {
  it("keeps each size's roles separate and lists one material", () => {
    const drafts = groupMaterials(["wood_albedo_4K.png", "wood_normal_4K.png", "wood_albedo_2K.png", "wood_normal_2K.png"]);
    expect(drafts).toHaveLength(2);
    const items = drafts.map((draft) => ({ draft, included: true }));
    const groups = materialGroups(items);
    expect(groups).toHaveLength(1);
    expect(items[groups[0][0]].draft.name).toBe("wood_4096");
    const entries = drafts.map((draft) => toEntry(draft, "p", "wood")).map((result) => {
      if (!("entry" in result)) throw new Error(result.problems.join(", "));
      return result.entry;
    });
    const [asset] = groupSeedVariants(entries);
    expect(asset.variants).toHaveLength(1);
    expect(asset.kind === "material" && asset.maps[0].file).toContain("4K");
    const low = asset.variants![0];
    expect(low.kind === "material" && low.maps[0].file).toContain("2K");
  });

  it("does not merge colour names", () => {
    const items = groupMaterials(["wood_red_albedo_2K.png", "wood_blue_albedo_2K.png"]).map((draft) => ({ draft, included: true }));
    expect(materialGroups(items)).toHaveLength(2);
  });
});
