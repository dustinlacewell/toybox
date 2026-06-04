/**
 * Typed wrappers over the Rust command surface. This and the other files in
 * `services/` are the only modules that import `@tauri-apps/api`. Components
 * call these functions, never `invoke` directly.
 */

import { invoke } from "@tauri-apps/api/core";

import type { Catalog, PackMeta, ThumbState } from "../domain/catalog";

export const scanLibrary = (forceReseed: boolean): Promise<Catalog> =>
  invoke("scan_library", { forceReseed });

export const loadCatalog = (): Promise<Catalog | null> =>
  invoke("load_catalog");

export const saveCatalog = (catalog: Catalog): Promise<void> =>
  invoke("save_catalog", { catalog });

export const libraryRoot = (): Promise<string> => invoke("library_root");

export const loadPacks = (): Promise<PackMeta[]> => invoke("load_packs");

export const resolveAssetPath = (relPath: string): Promise<string> =>
  invoke("resolve_asset_path", { relPath });

export const thumbDir = (): Promise<string> => invoke("thumb_dir");

export const thumbPath = (assetId: string): Promise<string> =>
  invoke("thumb_path", { assetId });

export const saveThumb = (assetId: string, pngBytes: Uint8Array): Promise<void> =>
  invoke("save_thumb", { assetId, pngBytes: Array.from(pngBytes) });

export const setThumbState = (
  assetId: string,
  state: ThumbState,
  error?: string,
): Promise<void> => invoke("set_thumb_state", { assetId, state, error: error ?? null });

export const listPendingThumbs = (): Promise<string[]> =>
  invoke("list_pending_thumbs");

export const clearThumbs = (): Promise<void> => invoke("clear_thumbs");

export interface ExportReport {
  written: string[];
  skipped: string[];
  warnings: string[];
}

export const exportCopy = (
  assetIds: string[],
  targetDir: string,
  preserveStructure: boolean,
): Promise<ExportReport> =>
  invoke("export_copy", { req: { assetIds, targetDir, preserveStructure } });

export const exportGlb = (
  assetIds: string[],
  targetDir: string,
  preserveStructure: boolean,
): Promise<ExportReport> =>
  invoke("export_glb", { req: { assetIds, targetDir, preserveStructure } });

export type PlacerFormat = "glb" | "copy";

export interface ExportPlacerArgs {
  assetIds: string[];
  /** Filesystem root files are written under (the Godot project dir). */
  targetDir: string;
  /** Project-relative subfolder + res:// prefix, e.g. "assets/exported". */
  subDir: string;
  preserveStructure: boolean;
  format: PlacerFormat;
  /** Filesystem path of the asset_library.json to create or merge. */
  libraryJsonPath: string;
}

export const exportPlacer = (args: ExportPlacerArgs): Promise<ExportReport> =>
  invoke("export_placer", { req: args });

export type Axis = "x" | "y" | "z";
export type Align = "min" | "center" | "max";

/** World-space AABB returned after an origin correction. */
export interface Aabb {
  min: [number, number, number];
  max: [number, number, number];
}

/**
 * Bake an origin correction into the asset's source .gltf: land the chosen
 * bounding-box point (min/center/max) on the local origin for one axis.
 * Returns the resulting world AABB.
 */
export const recenterAsset = (
  assetId: string,
  axis: Axis,
  align: Align,
): Promise<Aabb> => invoke("recenter_asset", { assetId, axis, align });
