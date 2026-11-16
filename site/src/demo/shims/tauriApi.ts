/**
 * Demo shim for services/tauriApi — the app's entire Rust command surface,
 * replaced with browser-native behaviour. The thumbnail pipeline is real: the
 * renderer (real app code) hands saveThumb its PNG bytes, which we keep in the
 * in-memory store so cards display genuine, live-rendered previews. Filesystem
 * operations (catalog persistence, export, origin-correction, reveal) have no
 * meaning without the library on disk and are inert here.
 */

import type { Catalog, PackMeta, ThumbState } from "@app/domain/catalog";
import { DEMO_ASSET_IDS, DEMO_PACKS, demoCatalog } from "../catalog";
import { putThumb } from "./thumbStore";
import { assetUrl } from "./assetUrl";

export const deleteAssets = async (assetIds:string[]):Promise<Catalog> => {
  const catalog=demoCatalog();return {...catalog,assets:catalog.assets.filter(asset=>!assetIds.includes(asset.id))};
};

export interface PreviewChoice { assetId: string; rigId: string; exact: boolean; }
export const compatiblePreviews = async (_assetId: string): Promise<PreviewChoice[]> => [];
export const readAnimationPreview = async (): Promise<unknown> => { throw new Error("The demo has no standalone animations."); };
export const readPreviewPair = readAnimationPreview;

export const readAssetGltf = async (assetId: string): Promise<unknown> => {
  const asset = demoCatalog().assets.find(asset => asset.id === assetId);
  if (!asset || asset.kind === "material") throw new Error("Scene asset not found");
  const response = await fetch(await assetUrl(asset.fileset.gltf));
  if (!response.ok) throw new Error("Could not read scene");
  return response.json();
};
export const readPreviewSource=async(relativePath:string,clipIndex:number|null):Promise<unknown>=>{
  const response=await fetch(await assetUrl(relativePath));if(!response.ok)throw Error(`Could not read ${relativePath}`);
  const doc=await response.json();if(clipIndex===null)delete doc.animations;else doc.animations=[doc.animations[clipIndex]];return doc;
};

// --- Thumbnails: the one genuinely live path ---------------------------------

export const saveThumb = async (assetId: string, png: Uint8Array): Promise<void> => {
  putThumb(assetId, png);
};

export const setThumbState = async (
  _assetId: string,
  _state: ThumbState,
  _error?: string,
): Promise<void> => {};

export const listPendingThumbs = async (): Promise<string[]> => [...DEMO_ASSET_IDS];

export const clearThumbs = async (): Promise<void> => {};
export const invalidateThumb = async (_assetId:string): Promise<void> => {};

export const thumbDir = async (): Promise<string> => "/demo-assets";

export const thumbPath = async (assetId: string): Promise<string> => assetId;

// --- Catalog / paths: served from the fixture, never persisted ---------------

export const scanLibrary = async (_forceReseed: boolean): Promise<Catalog> =>
  demoCatalog();

export const loadCatalog = async (): Promise<Catalog | null> => demoCatalog();

export const saveCatalog = async (_catalog: Catalog): Promise<void> => {};

export const libraryRoot = async (): Promise<string> => "polygon_prototype";

// The demo's library is the bundled fixture — always "configured", so the
// first-run picker never appears and re-pointing is a no-op.
export const getLibraryRoot = async (): Promise<string | null> => "polygon_prototype";

export const setLibraryRoot = async (_path: string): Promise<void> => {};

export const createLibrary = async (_path: string): Promise<void> => {};

// Plugin source-read primitives — inert in the browser (no filesystem to walk).
export const pluginReadDir = async (
  _sourceRoot: string,
  _path: string,
): Promise<{ name: string; relPath: string; isDir: boolean }[]> => [];

export const pluginReadBytes = async (
  _sourceRoot: string,
  _path: string,
): Promise<Uint8Array> => new Uint8Array();

// Settings / FBX conversion — no filesystem or process in the browser.
export const getFbx2gltfPath = async (): Promise<string | null> => null;
export const setFbx2gltfPath = async (_path: string | null): Promise<void> => {};
export const convertToGltf = async (
  _srcPath: string,
  _pack: string,
  _category: string,
  _stem: string,
): Promise<string[]> => [];

export const loadPacks = async (): Promise<PackMeta[]> => DEMO_PACKS;

export const resolveAssetPath = async (relPath: string): Promise<string> => relPath;

// --- Export / origin tools: inert in the browser -----------------------------

export interface ExportReport {
  written: string[];
  skipped: string[];
  warnings: string[];
}

const NOOP_REPORT: ExportReport = { written: [], skipped: [], warnings: [] };

export const exportCopy = async (): Promise<ExportReport> => NOOP_REPORT;
export const exportGlb = async (): Promise<ExportReport> => NOOP_REPORT;

export type PlacerFormat = "glb" | "copy";

export interface ExportPlacerArgs {
  assetIds: string[];
  targetDir: string;
  subDir: string;
  preserveStructure: boolean;
  format: PlacerFormat;
  libraryJsonPath: string;
}

export const exportPlacer = async (_args: ExportPlacerArgs): Promise<ExportReport> =>
  NOOP_REPORT;

export type Axis = "x" | "y" | "z";
export type Align = "min" | "center" | "max";

export interface Aabb {
  min: [number, number, number];
  max: [number, number, number];
}

export const recenterAsset = async (
  _assetId: string,
  _axis: Axis,
  _align: Align,
): Promise<Aabb> => ({ min: [0, 0, 0], max: [0, 0, 0] });
