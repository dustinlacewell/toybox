/**
 * The demo's fixture catalog: the bundled slice of the Polygon Prototype pack,
 * lifted into the app's real Catalog shape. Records are generated from the
 * actual bundled glTF filesets (_records.json); here we just decorate them with
 * the per-asset UserMeta / ThumbMeta / AnimationMeta the app expects.
 *
 * Thumbnails start "missing" — the real renderer fills them in live once the
 * demo mounts, exactly as the app does on first run.
 */

import type { Asset, Catalog, Category, PackMeta } from "@app/domain/catalog";
import records from "./_records.json";

interface Record {
  id: string;
  name: string;
  fileName: string;
  relPath: string;
  pack: string;
  category: string;
  fileset: { gltf: string; bin: string; textures: string[] };
  clipCount: number;
}

export const DEMO_PACKS: PackMeta[] = [
  { slug: "polygon_prototype", name: "Polygon Prototype", color: "#7ed321" },
];

const toAsset = (r: Record): Asset => ({
  id: r.id,
  name: r.name,
  fileName: r.fileName,
  relPath: r.relPath,
  pack: r.pack,
  category: r.category as Category,
  fileset: r.fileset,
  thumb: { state: "missing" },
  user: { favorite: false, tags: [] },
  animation: { clipCount: r.clipCount, clipNames: [] },
});

const ASSETS: Asset[] = (records as Record[]).map(toAsset);

export const DEMO_ASSET_IDS: string[] = ASSETS.map((a) => a.id);

export function demoCatalog(): Catalog {
  return {
    schemaVersion: 1,
    libraryRoot: "polygon_prototype",
    // Fresh copies so the demo store owns mutable favorite/thumb state.
    assets: ASSETS.map((a) => ({ ...a, thumb: { ...a.thumb }, user: { ...a.user } })),
  };
}
