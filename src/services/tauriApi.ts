/**
 * Typed wrappers over the Rust command surface. This and the other files in
 * `services/` are the only modules that import `@tauri-apps/api`. Components
 * call these functions, never `invoke` directly.
 */

import { invoke } from "@tauri-apps/api/core";

import type { Catalog, PackMeta, ThumbState } from "../domain/catalog";
import type { ExportReport, PluginUi, SeedEntryInput } from "@ldlework/toybox-sdk";

export type { ExportReport };

export const scanLibrary = (forceReseed: boolean): Promise<Catalog> =>
  invoke("scan_library", { forceReseed });

export const loadCatalog = (): Promise<Catalog | null> =>
  invoke("load_catalog");

export const saveCatalog = (catalog: Catalog): Promise<void> =>
  invoke("save_catalog", { catalog });

export const libraryRoot = (): Promise<string> => invoke("library_root");

/** The configured library root, or null if the user hasn't picked one yet. */
export const getLibraryRoot = (): Promise<string | null> =>
  invoke("get_library_root");

/** Adopt a user-chosen folder as the library root. Rejects (with a message)
 *  if the folder isn't a Toybox-style library. */
export const setLibraryRoot = (path: string): Promise<void> =>
  invoke("set_library_root", { path });

/** Scaffold a new, empty library in the chosen folder and adopt it. Rejects
 *  (with a message) if the folder isn't empty (and isn't already a library). */
export const createLibrary = (path: string): Promise<void> =>
  invoke("create_library", { path });

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

// --- Plugin system -----------------------------------------------------------

/** A discovered plugin's validated manifest + the abs path of its entry module. */
export interface PluginManifestDto {
  id: string;
  name: string;
  version: string;
  kind: "exporter" | "importer";
  entry: string;
  description?: string;
  permissions: { fsWrite?: boolean; fsRead?: boolean; rustExport?: boolean };
  fields: unknown[];
  ui?: PluginUi;
  entryAbsPath: string;
}

export const listPlugins = (): Promise<PluginManifestDto[]> => invoke("list_plugins");

/** Merge plugin-produced seed entries into the catalog (the importer commit).
 *  Opens the previously-closed inbound format. Returns the updated catalog. */
export const mergeSeedEntries = (entries: SeedEntryInput[]): Promise<Catalog> =>
  invoke("merge_seed_entries", { entries });

export const pluginReadText = (path: string): Promise<string> =>
  invoke("plugin_read_text", { path });

export const pluginExists = (path: string): Promise<boolean> =>
  invoke("plugin_exists", { path });

export const pluginWriteBytes = (
  authorizedRoot: string,
  path: string,
  bytes: Uint8Array,
): Promise<void> =>
  invoke("plugin_write_bytes", { authorizedRoot, path, bytes: Array.from(bytes) });

export const pluginWriteText = (
  authorizedRoot: string,
  path: string,
  text: string,
): Promise<void> => invoke("plugin_write_text", { authorizedRoot, path, text });

export const readAssetGltf = (assetId: string): Promise<unknown> =>
  invoke("read_asset_gltf", { assetId });

export const assembleGlbForAsset = (assetId: string): Promise<number[]> =>
  invoke("assemble_glb_for_asset", { assetId });

export const performAssetCopy = (
  assetId: string,
  targetDir: string,
  stem: string,
  preserveStructure: boolean,
): Promise<string[]> =>
  invoke("perform_asset_copy", { assetId, targetDir, stem, preserveStructure });

export const transcodeImage = (
  fileName: string,
  bytes: Uint8Array,
): Promise<{ mime: string; bytes: number[] }> =>
  invoke("transcode_image", { fileName, bytes: Array.from(bytes) });

export interface PlacerAssetDto {
  pack: string;
  category: string;
  favorite: boolean;
  tags: string[];
  resPath: string;
  name: string;
}

export const placerMergeFile = (
  libraryJsonPath: string,
  subDirRes: string,
  assets: PlacerAssetDto[],
): Promise<void> =>
  invoke("placer_merge_file", { libraryJsonPath, subDirRes, assets });

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
