import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createThumbLoader } from "./thumbs.js";

let made = 0;
const revoke = vi.fn();

beforeEach(() => {
  made = 0;
  revoke.mockClear();
  vi.stubGlobal("URL", {
    createObjectURL: vi.fn(() => `blob:${++made}`),
    revokeObjectURL: revoke,
  });
});
afterEach(() => vi.unstubAllGlobals());

/** A fake fs whose reads finish only when the test says so. */
function gatedFs() {
  const reads: Array<{ path: string; done: () => void; fail: (e: Error) => void }> = [];
  let inFlight = 0;
  let peak = 0;
  return {
    reads,
    peak: () => peak,
    fs: {
      readImageThumb: (_root: string, path: string, _size: number) =>
        new Promise<Uint8Array>((resolve, reject) => {
          peak = Math.max(peak, ++inFlight);
          reads.push({
            path,
            done: () => {
              inFlight--;
              resolve(new Uint8Array([1]));
            },
            fail: (e) => {
              inFlight--;
              reject(e);
            },
          });
        }),
    },
  };
}

const tick = () => new Promise((r) => setTimeout(r, 0));

describe("createThumbLoader", () => {
  it("reads the same (path,size) once and shares the result", async () => {
    const g = gatedFs();
    const loader = createThumbLoader(g.fs, "/root");
    const a = loader.load("a.png", 64);
    const b = loader.load("a.png", 64);
    await tick();
    expect(g.reads).toHaveLength(1);
    g.reads[0].done();
    expect(await a).toBe(await b);
  });

  it("reads again for a different size", async () => {
    const g = gatedFs();
    const loader = createThumbLoader(g.fs, "/root");
    void loader.load("a.png", 64);
    void loader.load("a.png", 128);
    await tick();
    expect(g.reads).toHaveLength(2);
  });

  it("never runs more than 4 reads at once", async () => {
    const g = gatedFs();
    const loader = createThumbLoader(g.fs, "/root");
    const all = Array.from({ length: 10 }, (_, i) => loader.load(`f${i}.png`, 64));
    await tick();
    expect(g.reads).toHaveLength(4);
    let finished = 0;
    while (finished < 10) {
      g.reads[finished].done();
      finished++;
      await tick();
    }
    await Promise.all(all);
    expect(g.reads).toHaveLength(10);
    expect(g.peak()).toBe(4);
  });

  it("rejects when the read fails and tries again next time", async () => {
    const g = gatedFs();
    const loader = createThumbLoader(g.fs, "/root");
    const first = loader.load("bad.png", 64);
    await tick();
    g.reads[0].fail(new Error("cannot decode"));
    await expect(first).rejects.toThrow("cannot decode");
    void loader.load("bad.png", 64);
    await tick();
    expect(g.reads).toHaveLength(2);
  });

  it("dispose revokes every URL made", async () => {
    const g = gatedFs();
    const loader = createThumbLoader(g.fs, "/root");
    const a = loader.load("a.png", 64);
    const b = loader.load("b.png", 64);
    await tick();
    g.reads[0].done();
    g.reads[1].done();
    const urls = [await a, await b];
    loader.dispose();
    expect(revoke.mock.calls.map((c) => c[0]).sort()).toEqual(urls.sort());
  });

  it("revokes a read that arrives after dispose and does not cache it", async () => {
    const g = gatedFs();
    const loader = createThumbLoader(g.fs, "/root");
    const a = loader.load("a.png", 64);
    await tick();
    loader.dispose();
    g.reads[0].done();
    const url = await a;
    expect(revoke).toHaveBeenCalledWith(url);
    void loader.load("a.png", 64);
    await tick();
    expect(g.reads).toHaveLength(2);
  });
});
