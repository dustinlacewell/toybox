/** Demo fixtures use the app catalog schema. Bundled thumbnails were generated
 * with the app renderer; preview preference changes refresh them normally. */
import type { Asset, Catalog, Category, PackMeta } from "@app/domain/catalog";
import records from "./_records.json";
import thumbnails from "./_thumbnails.json";
import samples from "./_samples.json";
import librarySamples from "./_library-samples.json";

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
  { slug: "toybox_samples", name: "Toybox samples", color: "#4a90e2" },
];

const toAsset = (r: Record): Asset => ({
  kind: "model",
  id: r.id,
  name: r.name,
  fileName: r.fileName,
  relPath: r.relPath,
  pack: r.pack,
  category: r.category as Category,
  fileset: r.fileset,
  thumb: { state: "missing" },
  user: { favorite: false, tags: ["source: Polygon Prototype","publisher: Synty"], provenance:{source:"Polygon Prototype",publisher:"Synty"} },
  animation: { clipCount: r.clipCount, clipNames: [] },
});

const ASSETS: Asset[] = [...(records as Record[]).map(toAsset),...samples.assets as Asset[], ...librarySamples.assets as Asset[]];

export const DEMO_ASSET_IDS: string[] = ASSETS.map((a) => a.id);

export function demoCatalog(): Catalog {
  return {
    schemaVersion: 1,
    rigs: samples.rigs,
    libraryRoot: "polygon_prototype",
    // Fresh copies so the demo store owns mutable favorite/thumb state.
    assets: ASSETS.map((a) => ({ ...a, thumb: { ...a.thumb, state: a.kind === "audio" || a.id in thumbnails ? "ready" : "missing" }, user: { ...a.user } })),
  };
}
