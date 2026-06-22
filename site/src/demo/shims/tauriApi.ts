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
