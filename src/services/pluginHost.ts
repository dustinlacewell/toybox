/**
 * Builds the `ctx` a plugin's `run` receives: the host API (catalog reads +
 * Rust export primitives), the jailed fs API, the report builder, and the
 * abort signal. This is the only plugin-system module that touches the Rust
 * command surface (via `tauriApi`) — plugins never see `invoke`.
 *
 * Permission gating: methods the plugin's manifest didn't declare are replaced
 * with stubs that throw. That is advisory — the real filesystem boundary is the
 * Rust path jail behind `plugin_write_*` (a write outside the run's authorized
 * target dir is rejected regardless of what the manifest claims).
 */

import {
  createReportBuilder,
  type ExportCtx,
  type FsApi,
  type HostApi,
  type PluginConfig,
  type PluginPermissions,
} from "@ldlework/toybox-sdk";
import { isSceneAsset, type Asset, type SceneAsset } from "../domain/catalog";
import type { PluginManifestDto } from "./tauriApi";
import { extensionLookup } from "./pluginExtensions";
import { buildPluginServices } from "./pluginServices";
import { cachedAsset, ensureAssets } from "../state/assetCache";
import { useStore } from "../state/store";
import { assetUrl } from "./assetUrl";
import { pickDirectory } from "./pickDirectory";
import { pickSaveFile } from "./pickSaveFile";
import {
  assembleGlbForAsset,
  performAssetCopy,
  previewExport,
  placerMergeFile,
  pluginCopyFile,
  pluginImageThumb,
  pluginReadBytes,
  pluginReadDir,
  pluginWriteBytes,
  pluginWriteText,
  readAssetGltf,
  transcodeImage,
  type PlacerAssetDto,
} from "./tauriApi";

/** Assemble the full `ctx` for one plugin run. */
export function buildExportCtx(
  manifest: PluginManifestDto,
  config: PluginConfig,
  signal: AbortSignal,
): ExportCtx {
  const perms: PluginPermissions = manifest.permissions ?? {};
  return {
    host: buildHostApi(manifest.id, perms),
    fs: buildFsApi(manifest.id, perms),
    services: buildPluginServices(manifest.id, perms),
    extensions: extensionLookup,
    config,
    report: createReportBuilder(),
    signal,
  };
}

/** Resolve this run's selection without dropping material records. The window holds no catalog: the items are
 *  the ones it has loaded (the run's selection is fetched first, see `loadSelectedAssets`). */
export function selectedAssets(selectedIds: string[]): Asset[] {
  return selectedIds.map((id) => cachedAsset(id)).filter((a): a is Asset => !!a);
}

/** The assets with these ids, fetched by id when the window does not hold them yet. */
export async function loadSelectedAssets(selectedIds: string[]): Promise<Asset[]> {
  await ensureAssets(selectedIds);
  return selectedAssets(selectedIds);
}

/** An item or variant the window has loaded (a plugin run's selection always is). */
export function catalogAsset(id: string): Asset | undefined {
  return cachedAsset(id);
}

/** An item or variant by id, fetched when it is not loaded. */
export async function fetchAsset(id: string): Promise<Asset | undefined> {
  await ensureAssets([id]);
  return cachedAsset(id);
}

/** Model-only helper for glTF-specific host calls. */
export function modelAsset(id: string): SceneAsset | undefined {
  const asset = catalogAsset(id);
  if (asset && !isSceneAsset(asset)) {
    throw new Error(`asset ${id} is a ${asset.kind}; this operation requires a model`);
  }
  return asset;
}

function buildHostApi(pluginId: string, perms: PluginPermissions): HostApi {
  const gatedExport = gate(pluginId, "rustExport", perms.rustExport);

  return {
    previewExport,
    getSelectedAssets: () => {
      const ids = [...useStore.getState().selection];
      return selectedAssets(ids);
    },
    getAsset: (id) => catalogAsset(id),
    getSelectedAssetsAsync: () => loadSelectedAssets([...useStore.getState().selection]),
    getAssetAsync: (id) => fetchAsset(id),
    getParsedGltf: (id) => readAssetGltf(id),
    getAssetUrls: async (id) => {
      await fetchAsset(id);
      const asset = modelAsset(id);
      if (!asset) throw new Error(`unknown asset: ${id}`);
      const [gltf, bin, ...textures] = await Promise.all([
        assetUrl(asset.library, asset.fileset.gltf),
        assetUrl(asset.library, asset.fileset.bin),
        ...asset.fileset.textures.map((texture) => assetUrl(asset.library, texture)),
      ]);
      return { gltf, bin, textures };
    },
    assembleGlb: gatedExport(async (id: string) =>
      Uint8Array.from(await assembleGlbForAsset(id)),
    ),
    performCopy: gatedExport(
      (id: string, targetDir: string, stem: string, preserve: boolean) =>
        performAssetCopy(id, targetDir, stem, preserve),
    ),
    placerMerge: gatedExport(
      (libraryJsonPath: string, subDirRes: string, assets: PlacerAssetDto[]) =>
        placerMergeFile(libraryJsonPath, subDirRes, assets),
    ),
    transcodeImage: gatedExport(async (fileName: string, bytes: Uint8Array) => {
      const out = await transcodeImage(fileName, bytes);
      return { mime: out.mime, bytes: Uint8Array.from(out.bytes) };
    }),
  };
}

/** The jailed fs an importer panel and an exporter run both receive. Reads are
 *  gated on `fsRead`, writes on `fsWrite`; an ungranted method throws when called
 *  (advisory — the real boundary is the Rust path jail). Exported so the import
 *  drawer can build the same fs for an importer panel. */
export function buildFsApi(pluginId: string, perms: PluginPermissions): FsApi {
  const gatedRead = gate(pluginId, "fsRead", perms.fsRead);
  const gatedWrite = gate(pluginId, "fsWrite", perms.fsWrite);
  return {
    readDir: gatedRead((sourceRoot: string, path: string) =>
      pluginReadDir(sourceRoot, path),
    ),
    readBytes: gatedRead((sourceRoot: string, path: string) =>
      pluginReadBytes(sourceRoot, path),
    ),
    readImageThumb: gatedRead((sourceRoot: string, path: string, maxSize: number) =>
      pluginImageThumb(sourceRoot, path, maxSize),
    ),
    copyFile: gatedRead(gatedWrite((sourceRoot: string, path: string, root: string, target: string) =>
      pluginCopyFile(sourceRoot, path, root, target),
    )),
    writeBytes: gatedWrite((root: string, path: string, bytes: Uint8Array) =>
      pluginWriteBytes(root, path, bytes),
    ),
    writeText: gatedWrite((root: string, path: string, text: string) =>
      pluginWriteText(root, path, text),
    ),
    writeJson: gatedWrite((root: string, path: string, value: unknown) =>
      pluginWriteText(root, path, JSON.stringify(value, null, 2)),
    ),
    pickDirectory,
    pickSaveFile,
  };
}

/** Wrap a method so that, unless `granted`, calling it throws a clear
 *  permission error naming the plugin and the missing capability. */
export function gate(pluginId: string, capability: keyof PluginPermissions, granted?: boolean) {
  return function <A extends unknown[], R>(fn: (...args: A) => R) {
    if (granted) return fn;
    return (..._args: A): R => {
      throw new Error(`plugin '${pluginId}' lacks permission: ${capability}`);
    };
  };
}
