import { describe, expect, it, vi } from "vitest";
import type { MaterialDraft } from "./draft.js";
import { previewUrlResolver } from "./previewSource.js";

const draft: MaterialDraft = {
  name: "Cobble",
  dir: "d",
  files: [
    { relPath: "d/a.png", fileName: "a.png", use: { use: "albedo" } },
    { relPath: "d/sub/n.png", fileName: "n.png", use: { use: "unassigned" } },
  ],
};

describe("previewUrlResolver", () => {
  it("loads the draft file with that name by its relPath", async () => {
    const load = vi.fn(async (rel: string) => `url:${rel}`);
    const resolve = previewUrlResolver(draft, load);
    await expect(resolve("n.png")).resolves.toBe("url:d/sub/n.png");
    expect(load).toHaveBeenCalledWith("d/sub/n.png");
  });

  it("rejects for an unknown file name without loading", async () => {
    const load = vi.fn(async (rel: string) => rel);
    await expect(previewUrlResolver(draft, load)("nope.png")).rejects.toThrow("nope.png");
    expect(load).not.toHaveBeenCalled();
  });
});
