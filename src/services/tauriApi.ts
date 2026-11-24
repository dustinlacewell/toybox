/**
 * Typed wrappers over the Rust command surface. This and the other files in
 * `services/` are the only modules that import `@tauri-apps/api`. Components
 * call these functions, never `invoke` directly.
 *
 * This is also where library identity is translated (see `domain/libraryId`).
 * The backend works on ONE library per call with raw asset ids; the app holds
 * one merged catalog of qualified ids (`<library>::<raw>`). Everything that
 * returns ids or catalogs qualifies them here, and everything that takes ids
 * groups them by library and sends raw ids with the library id.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { Asset, AssetKind, LibraryInfo, PackMeta, ThumbState } from "../domain/catalog";
import type { LibrarySummary } from "./browse";
import type { RemoteRole } from "../domain/remoteLibrary";
import { failedGroup, mergeTransferReports, overallProgress, planTransfer, type MergedTransferReport, type OnConflict, type TransferMode, type TransferReport } from "../domain/transfer";
import { groupIds, mergeReports, parseQualified, qualify, qualifyAsset, qualifyKeys } from "../domain/libraryId";
import type { AssetMetadataEdit } from "../domain/libraryTags";
import type { AssetTypeDeclaration, DirEntry, ExportReport, PluginPermissions, PluginUi, SeedEntryInput, SettingSpec } from "@ldlework/toybox-sdk";

export type { ExportReport };

/** A multi-library call that failed part-way: `done` holds what the earlier
 *  libraries had already completed, so the caller can still show it. */
export class PartialLibraryError<T> extends Error {
  constructor(message: string, readonly done: T[]) { super(message); }
}

/** Run `call` once per library, in order. A failure after earlier successes
 *  is reported with those results attached. */
async function eachLibrary<T>(groups: Map<string, string[]>, call: (library: string, ids: string[]) => Promise<T>): Promise<T[]> {
  const done: T[] = [];
  for (const [library, ids] of groups) {
    try { done.push(await call(library, ids)); }
    catch (failure) {
      if (done.length) throw new PartialLibraryError(String(failure), done);
      throw failure;
    }
  }
  return done;
}

// --- Libraries ---------------------------------------------------------------

export const listLibraries = (): Promise<LibraryInfo[]> => invoke("list_libraries");

/** Register an existing folder. Rejects (with a message) if it isn't a Toybox
 *  library or is already registered. `name` defaults to the folder name. */
export const addLibrary = (path: string, name?: string): Promise<LibraryInfo> =>
  invoke("add_library", { path, name: name ?? null });

/** Scaffold a new, empty library in the chosen folder and register it. Rejects
 *  (with a message) if the folder isn't empty (and isn't already a library). */
export const createLibrary = (path: string, name?: string): Promise<LibraryInfo> =>
  invoke("create_library", { path, name: name ?? null });

export const renameLibrary = (library: string, name: string): Promise<LibraryInfo> =>
  invoke("rename_library", { library, name });

/** Unregister a library. Its folder and files are left alone. */
export const removeLibrary = (library: string): Promise<void> => invoke("remove_library", { library });

export const setDefaultLibrary = (library: string): Promise<void> => invoke("set_default_library", { library });

// --- Remote libraries -----------------------------------------------------------

/** What a server says about itself and the token it was given (nothing is saved). */
export interface RemoteProbe {
  /** The normalised address. */
  url: string;
  /** Set when the address is not encrypted. */
  warning: string | null;
  serverName: string;
  role: RemoteRole;
  /** A sentence about the server's and this app's versions, when they differ. */
  notice?: string | null;
}

/** Check an address and token: the server's name and the role the token grants. */
export const probeRemoteLibrary = (address: string, token: string): Promise<RemoteProbe> =>
  invoke("probe_remote_library", { address, token });

/** Add a remote library. The token goes to the system credential store, never to settings. */
export const addRemoteLibrary = (address: string, token: string, name?: string): Promise<LibraryInfo> =>
  invoke("add_remote_library", { address, token, name: name ?? null });

/** Use another access token for a remote library (checked with the server first). */
export const changeRemoteToken = (library: string, token: string): Promise<LibraryInfo> =>
  invoke("change_remote_token", { library, token });

/** "Try again": ask the server again and report the library's state. */
export const reconnectRemoteLibrary = (library: string): Promise<LibraryInfo> =>
  invoke("reconnect_remote_library", { library });

/** Bytes of files downloaded from a remote library. */
export const remoteCacheSize = (library: string): Promise<number> => invoke("remote_cache_size", { library });

/** Delete the downloaded files of a remote library; resolves to the bytes freed. */
export const clearRemoteCache = (library: string): Promise<number> => invoke("clear_remote_cache", { library });

/** Make the files of these items (qualified ids) available on this computer; a
 *  no-op for items of a local library. `includeSources` also fetches the source
 *  files kept with an item. */
export const ensureAssetFiles = async (assetIds: string[], includeSources = false): Promise<void> => {
  for (const [library, ids] of groupIds(assetIds)) await invoke("ensure_asset_files", { library, assetIds: ids, includeSources });
};

/** One access token of a server (the secret is never part of this). */
export interface RemoteToken {
  id: string;
  name: string;
  role: RemoteRole;
  /** Seconds since the Unix epoch. */
  createdAt: number;
  lastUsedAt: number | null;
}

export const listRemoteTokens = (library: string): Promise<RemoteToken[]> => invoke("list_remote_tokens", { library });

/** Make a token. The secret in the answer is shown to the user once and never again. */
export const createRemoteToken = (library: string, name: string, role: RemoteRole): Promise<RemoteToken & { token: string }> =>
  invoke("create_remote_token", { library, name, role });

export const revokeRemoteToken = (library: string, tokenId: string): Promise<void> => invoke("revoke_remote_token", { library, tokenId });

/** Progress of a large upload or download of a remote library. */
export interface LibraryProgress {
  library: string;
  operation: "upload" | "download";
  doneFiles: number;
  totalFiles: number;
  doneBytes: number;
  totalBytes: number;
  label?: string;
}

/** Follow upload / download progress; returns the function that stops following. */
export async function onLibraryProgress(listener: (progress: LibraryProgress) => void): Promise<() => void> {
  return listen<LibraryProgress>("library-progress", (event) => listener(event.payload));
}

// --- Content fingerprints (duplicate detection across libraries) ----------------

/** How far the background fingerprinting of one local library is (`running: false`: finished). */
export interface FingerprintProgress { library: string; done: number; total: number; running: boolean; }

/** Start (or ask again for) the background pass that fingerprints items of the local libraries
 *  that have none yet. Returns at once; call it after the catalogs are loaded. */
export const startFingerprinting = (): Promise<void> => invoke("start_fingerprinting");

export const fingerprintStatus = (): Promise<FingerprintProgress[]> => invoke("fingerprint_status");

/** Follow the fingerprinting: progress per library, and the fingerprints themselves as they are
 *  computed (`{ raw asset id: hash }`, no rescan needed). Returns the function that stops following. */
export async function onFingerprints(handlers: {
  progress: (progress: FingerprintProgress) => void;
  hashes: (library: string, hashes: Record<string, string>) => void;
}): Promise<() => void> {
  const stops = await Promise.all([
    listen<FingerprintProgress>("fingerprint-progress", (event) => handlers.progress(event.payload)),
    listen<{ library: string; hashes: Record<string, string> }>("fingerprint-hashes", (event) => handlers.hashes(event.payload.library, event.payload.hashes)),
  ]);
  return () => stops.forEach((stop) => stop());
}

// --- Libraries (one library per call; results are qualified). Reading them is `services/browse`. ------------

/** The explicit rescan: rebuild a library's catalog from its manifest (a remote library downloads it again). */
export const scanLibrary = (library: string, forceReseed: boolean): Promise<LibrarySummary> => invoke("scan_library", { library, forceReseed });

/** What a delete did in one library: the ids (qualified) that are gone, and where the library stands. */
export interface DeleteResult { library: string; deleted: string[]; summary: LibrarySummary }

/** Delete items; each library deletes its own. */
export const deleteAssets = (assetIds: string[]): Promise<DeleteResult[]> =>
  eachLibrary(groupIds(assetIds), async (library, ids) => {
    const reply = await invoke<{ deleted: string[]; summary: LibrarySummary }>("delete_assets", { library, assetIds: ids });
    return { library, deleted: reply.deleted.map((id) => qualify(library, id)), summary: reply.summary };
  });

/**
 * Change the tags and provenance of items (saved in the library itself). Each library makes its own edits, a remote
 * one through its server; the answer is the touched top-level items as the cache
 * now holds them, qualified, ready for `applyUpdatedAssets`.
 */
export async function updateAssetMetadata(edits: AssetMetadataEdit[]): Promise<Asset[]> {
  const groups = new Map<string, AssetMetadataEdit[]>();
  for (const edit of edits) {
    const parsed = parseQualified(edit.id);
    if (!parsed) throw new Error(`Not a library item id: ${edit.id}`);
    groups.set(parsed.library, [...(groups.get(parsed.library) ?? []), { ...edit, id: parsed.id }]);
  }
  const done: Asset[][] = [];
  for (const [library, list] of groups) {
    try { done.push((await invoke<Asset[]>("update_asset_metadata", { library, edits: list })).map((asset) => qualifyAsset(asset, library))); }
    catch (failure) {
      if (done.length) throw new PartialLibraryError(String(failure), done.flat());
      throw failure;
    }
  }
  return done.flat();
}

/** What to do with a transfer and how to follow it. */
export interface TransferRequest {
  /** The items, qualified ids; they may come from several libraries. */
  assets: Asset[];
  toLibrary: string;
  mode: TransferMode;
  /** An item already in the target library: leave it (`skip`) or overwrite it (`replace`). */
  onConflict: OnConflict;
  /** Also take the animation clips extracted from the selected models. */
  includeClips?: boolean;
  onProgress?: (progress: { done: number; total: number }) => void;
}

/**
 * Copy or move items into another library. The backend handles one source
 * library per call, so the selection is grouped by source; what already sits in
 * the target is reported as skipped without a call. A source whose call fails
 * as a whole reports all its items as failed, so the others still go through.
 * The caller refreshes the libraries named by `affectedLibraries`.
 */
export async function transferAssets(request: TransferRequest, targetName: string): Promise<MergedTransferReport> {
  const { groups, alreadyThere } = planTransfer(request.assets, request.toLibrary);
  const sources = [...groups.keys()];
  const totals = sources.map((library) => groups.get(library)!.length);
  const report = (finished: number, running: { done: number; total: number } | null) =>
    request.onProgress?.(overallProgress(totals, finished, running));
  report(0, null);
  const unlisten = await listen<{ fromLibrary: string; toLibrary: string; done: number; total: number }>("transfer-progress", (event) => {
    const index = sources.indexOf(event.payload.fromLibrary);
    if (index >= 0 && event.payload.toLibrary === request.toLibrary) report(index, event.payload);
  });
  const parts: { library: string; report: TransferReport }[] = [];
  try {
    for (const [index, library] of sources.entries()) {
      const assets = groups.get(library)!;
      try {
        const ids = groupIds(assets.map((asset) => asset.id)).get(library)!;
        parts.push({ library, report: await invoke<TransferReport>("transfer_assets", {
          fromLibrary: library, toLibrary: request.toLibrary, ids, mode: request.mode, onConflict: request.onConflict, includeClips: !!request.includeClips,
        }) });
      } catch (failure) {
        parts.push({ library, report: failedGroup(assets, String(failure).replace(/^Error: /, "")) });
      }
      report(index + 1, null);
    }
  } finally {
    unlisten();
  }
  return mergeTransferReports(parts, alreadyThere, targetName);
}

export const libraryRoot = (library: string): Promise<string> => invoke("library_root", { library });

/** Convert one `.fbx` into loose glTF under library/<pack>/<category>/ of
 *  `library`, returning the library-relative paths written (`.gltf` first). */
export const convertToGltf = (
  library: string,
  srcPath: string,
  pack: string,
  category: string,
  stem: string,
): Promise<string[]> =>
  invoke("convert_to_gltf", { library, srcPath, pack, category, stem });

/** The configured FBX2glTF executable path, or null if unset. */
export const getFbx2gltfPath = (): Promise<string | null> =>
  invoke("get_fbx2gltf_path");

/** Set (or clear, with null) the FBX2glTF executable path. */
export const setFbx2gltfPath = (path: string | null): Promise<void> =>
  invoke("set_fbx2gltf_path", { path });

/** Pack identity of every readable library, merged (first one wins per slug). */
export const loadPacks = async (): Promise<PackMeta[]> => {
  const packs = new Map<string, PackMeta>();
  for (const library of (await listLibraries()).filter((l) => l.available))
    for (const pack of await invoke<PackMeta[]>("load_packs", { library: library.id })) if (!packs.has(pack.slug)) packs.set(pack.slug, pack);
  return [...packs.values()];
};

export const resolveAssetPath = (library: string, relPath: string): Promise<string> =>
  invoke("resolve_asset_path", { library, relPath });

export const thumbDir = (library: string): Promise<string> => invoke("thumb_dir", { library });

// Thumbnails are addressed by qualified asset id: the library picks the folder.
const inLibrary = (assetId: string) => {
  const parsed = parseQualified(assetId);
  if (!parsed) throw new Error(`Not a library item id: ${assetId}`);
  return { library: parsed.library, assetId: parsed.id };
};

export const thumbPath = (assetId: string): Promise<string> => invoke("thumb_path", inLibrary(assetId));

export const saveThumb = (assetId: string, pngBytes: Uint8Array): Promise<void> =>
  invoke("save_thumb", { ...inLibrary(assetId), pngBytes: Array.from(pngBytes) });

export const setThumbState = (
  assetId: string,
  state: ThumbState,
  error?: string,
): Promise<void> => invoke("set_thumb_state", { ...inLibrary(assetId), state, error: error ?? null });

export const clearThumbs = async (assetIds?: string[], preserveExisting = false): Promise<void> => {
  const groups: Map<string, string[] | null> = assetIds
    ? groupIds(assetIds)
    : new Map((await listLibraries()).filter((l) => l.available).map((l) => [l.id, null]));
  for (const [library, ids] of groups) await invoke("clear_thumbs", { library, assetIds: ids, preserveExisting });
};
export const invalidateThumb = (assetId: string): Promise<void> => invoke("invalidate_thumb", inLibrary(assetId));

/** Run a native exporter once per library involved and merge the reports. */
const exportPerLibrary = async (command: string, assetIds: string[], targetDir: string, preserveStructure: boolean): Promise<ExportReport> =>
  mergeReports(await eachLibrary(groupIds(assetIds), (library, ids) =>
    invoke<ExportReport>(command, { library, req: { assetIds: ids, targetDir, preserveStructure } })));

export const exportCopy = (assetIds: string[], targetDir: string, preserveStructure: boolean): Promise<ExportReport> =>
  exportPerLibrary("export_copy", assetIds, targetDir, preserveStructure);

export const exportGlb = (assetIds: string[], targetDir: string, preserveStructure: boolean): Promise<ExportReport> =>
  exportPerLibrary("export_glb", assetIds, targetDir, preserveStructure);

// --- Plugin system -----------------------------------------------------------

/** A discovered plugin's validated manifest + the abs path of its entry module. */
export interface PluginManifestDto {
  icon?: string;
  assetIcons?: Partial<Record<AssetKind,string>>;
  id: string;
  name: string;
  version: string;
  /** Empty when the plugin has no entry module. */
  entry: string;
  description?: string;
  /** The types the plugin works with. `"*"` (native exporters only) means any. */
  assetKinds: AssetKind[];
  /** The library types an `assetType` plugin defines. */
  assetTypes?: AssetTypeDeclaration[];
  permissions: PluginPermissions;
  fields: unknown[];
  /** Settings the plugin asks the user for, shown under it in Settings. */
  settings?: SettingSpec[];
  ui?: PluginUi;
  entryAbsPath: string;
  /** False when the user has turned the plugin off. Absent means on. */
  enabled?: boolean;
  /** The plugins this one needs, each with the version range it accepts. */
  dependencies?: Record<string, string>;
  /** Direct dependencies that are not usable. Empty when the plugin can load. */
  blockedBy?: PluginBlock[];
  /** Ids of switched-on plugins that directly need this one. */
  requiredBy?: string[];
  /** Retired plugin ids this plugin replaces; library items that record one
   *  of them in `origin.plugin` follow this plugin. */
  replaces?: string[];
}

export interface PluginBlock {
  id: string;
  reason: "missing" | "version" | "disabled" | "blocked" | "cycle";
  needs?: string;
  found?: string;
}

export const listPlugins = (): Promise<PluginManifestDto[]> => invoke("list_plugins");
/** A plugin's settings: what the user chose, else each declared default. */
export const getPluginSettings = (pluginId: string): Promise<Record<string, unknown>> => invoke("get_plugin_settings", { pluginId });
/** Store one declared setting of a plugin; `null` returns it to its default. */
export const setPluginSetting = (pluginId: string, key: string, value: unknown): Promise<void> => invoke("set_plugin_setting", { pluginId, key, value: value ?? null });
export const pluginLaunchTool = (pluginId: string, setting: string, args: string[]): Promise<void> => invoke("plugin_launch_tool", { pluginId, setting, args });
export const pluginHostProcessId = (): Promise<number> => invoke("plugin_host_process_id");
export const pluginLocalPortOpen = (pluginId: string, port: number): Promise<boolean> => invoke("plugin_local_port_open", { pluginId, port });
export const pluginLocalJson = (pluginId: string, port: number, message: unknown, timeoutMs: number): Promise<unknown> => invoke("plugin_local_json", { pluginId, port, message, timeoutMs });
export const pluginScratchDir = (pluginId: string, clear: boolean): Promise<string> => invoke("plugin_scratch_dir", { pluginId, clear });
/** Run a plugin's tool setting to completion and return how it ended. */
export const pluginRunTool = (pluginId: string, setting: string, args: string[], timeoutMs: number): Promise<{ code: number; stdout: string; stderr: string }> => invoke("plugin_run_tool", { pluginId, setting, args, timeoutMs });
/** Turn an installed plugin on or off and return the ids whose switch changed
 *  (turning one on also turns on what it needs). Reload the registry to apply it. */
export const setPluginEnabled = (id: string, enabled: boolean): Promise<string[]> => invoke("set_plugin_enabled", { id, enabled });
export const previewExport = async (assetIds: string[], format: string, preserveStructure: boolean): Promise<string[]> =>
  [...new Set((await eachLibrary(groupIds(assetIds), (library, ids) =>
    invoke<string[]>("preview_export", { library, assetIds: ids, format, preserveStructure }))).flat())];
/** Content hashes by qualified asset id. */
export const assetFingerprints = async (assetIds: string[], allVariants: boolean): Promise<Record<string, string>> =>
  Object.assign({}, ...(await eachLibrary(groupIds(assetIds), async (library, ids) =>
    qualifyKeys(library, await invoke<Record<string, string>>("asset_fingerprints", { library, assetIds: ids, allVariants })))));

export const getSavedExports = (): Promise<import("../domain/savedExport").SavedExport[]> => invoke("get_saved_exports");
export const setSavedExports = (exports: import("../domain/savedExport").SavedExport[]): Promise<void> => invoke("set_saved_exports", { exports });

/** Merge plugin-produced seed entries into the catalog (the importer commit).
 *  Opens the previously-closed inbound format. Says where the library stands afterwards. */
export const mergeSeedEntries = (library: string, entries: SeedEntryInput[]): Promise<LibrarySummary> =>
  invoke("merge_seed_entries", { library, entries });

export const pluginReadText = (path: string): Promise<string> =>
  invoke("plugin_read_text", { path });

export const pluginExists = (path: string): Promise<boolean> =>
  invoke("plugin_exists", { path });

/** List one directory level under a user-authorized source root. `relPath` is
 *  source-root-relative, so the caller recurses by passing it back as `path`. */
export const pluginReadDir = (sourceRoot: string, path: string): Promise<DirEntry[]> =>
  invoke("plugin_read_dir", { sourceRoot, path });

export const pluginReadBytes = async (
  sourceRoot: string,
  path: string,
): Promise<Uint8Array> =>
  new Uint8Array(await invoke<ArrayBuffer>("plugin_read_bytes", { sourceRoot, path }));

/** The SHA-256 (lower-case hex) of a file under a source root, hashed natively. */
export const pluginHashFile = (sourceRoot: string, path: string): Promise<string> =>
  invoke<string>("plugin_hash_file", { sourceRoot, path });

/** The size in bytes of a file under a source root, from its metadata (never read). */
export const pluginFileSize = (sourceRoot: string, path: string): Promise<number> =>
  invoke<number>("plugin_file_size", { sourceRoot, path });

/** Copy a file natively from a source root into an authorized root, so large
 *  media never crosses the IPC boundary. */
export const pluginCopyFile = (
  sourceRoot: string,
  path: string,
  authorizedRoot: string,
  target: string,
): Promise<void> => invoke("plugin_copy_file", { sourceRoot, path, authorizedRoot, target });

/** PNG bytes of an image under a source root, scaled to fit `maxSize`. */
export const pluginImageThumb = async (
  sourceRoot: string,
  path: string,
  maxSize: number,
): Promise<Uint8Array> =>
  Uint8Array.from(await invoke<number[]>("plugin_image_thumb", { sourceRoot, path, maxSize }));

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
  invoke("read_asset_gltf", inLibrary(assetId));
export const readPreviewPair=(library:string,animationPath:string,clipIndex:number,modelPath:string):Promise<unknown>=>
  invoke("read_preview_pair",{library,animationPath,clipIndex,modelPath});
export const readPreviewSource=(library:string,relativePath:string,clipIndex:number|null):Promise<unknown>=>
  invoke("read_preview_source",{library,relativePath,clipIndex});

export interface PreviewChoice { assetId: string; rigId: string; exact: boolean; }
export const compatiblePreviews = async (assetId: string): Promise<PreviewChoice[]> => {
  const { library, assetId: raw } = inLibrary(assetId);
  return (await invoke<PreviewChoice[]>("compatible_previews", { library, assetId: raw })).map((choice) => ({ ...choice, assetId: qualify(library, choice.assetId) }));
};
/** An animation and the model to show it on must be in the same library. */
export const readAnimationPreview = (animationId: string, modelId: string): Promise<unknown> => {
  const animation = inLibrary(animationId), model = inLibrary(modelId);
  if (animation.library !== model.library) return Promise.reject(new Error("An animation can only be shown on a model from its own library."));
  return invoke("read_animation_preview", { library: animation.library, animationId: animation.assetId, modelId: model.assetId });
};

export const assembleGlbForAsset = (assetId: string): Promise<number[]> =>
  invoke("assemble_glb_for_asset", inLibrary(assetId));

export const performAssetCopy = (
  assetId: string,
  targetDir: string,
  stem: string,
  preserveStructure: boolean,
): Promise<string[]> =>
  invoke("perform_asset_copy", { ...inLibrary(assetId), targetDir, stem, preserveStructure });

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
): Promise<Aabb> => invoke("recenter_asset", { ...inLibrary(assetId), axis, align });

// --- The command line (tbx) -----------------------------------------------------

/** Whether `tbx` (shipped with the app) is on the user's PATH, and what is stale. */
export interface CliStatus {
  bundled: string | null;
  bundledVersion: string | null;
  onPath: string | null;
  onPathVersion: string | null;
  appVersion: string;
  upToDate: boolean;
  target: string | null;
  targetOnPath: boolean;
  /** A package or installer already put it on the PATH. */
  managed: boolean;
  message: string;
}

export interface CliChange {
  program: string | null;
  folder: string | null;
  changed: boolean;
  note: string | null;
}

export const cliStatus = (): Promise<CliStatus> => invoke("cli_status");
export const installCli = (): Promise<CliChange> => invoke("cli_install");
export const uninstallCli = (): Promise<CliChange> => invoke("cli_uninstall");
