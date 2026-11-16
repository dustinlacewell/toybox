/**
 * `@ldlework/toybox-sdk` — the plugin authoring SDK for Toybox.
 *
 *   import { defineExportPanel, type ExportPanelCtx } from "@ldlework/toybox-sdk";
 *
 * This package is the single source of truth for the plugin contract: the
 * manifest shape, the `ctx` objects a plugin's `run`/panels receive, the
 * declarative field schema, and the `define*` authoring helpers. Plugin authors
 * install it for types; at runtime the host's import map redirects the bare
 * `@ldlework/toybox-sdk` specifier to the host's bundled chunk, so the plugin
 * shares the host's instances (one React, one SDK) rather than bundling a copy.
 *
 * Self-contained on purpose: no imports from the host app. Design-system
 * primitives live in the sibling `@ldlework/toybox-sdk/ui` entry.
 */

import type { ComponentType, ReactNode } from "react";
export { variantInfo, groupSeedVariants } from "./variants.js";
export { defineRigAdapter, type RigProfile, type RigAdapterPlugin } from "./rig.js";

// --- catalog view ------------------------------------------------------------

/** Original source information, independent of library storage paths. */
export interface ImportProvenance {
  filesystemPath?: string;
  source?: string;
  publisher?: string;
  url?: string;
  license?: string;
}

/** The read-only view of a catalog asset a plugin sees. A self-contained mirror
 *  of the host's `Asset` (minus host-only fields like thumbnail state); the host
 *  passes assets typed as this. */
export interface AssetViewBase {
  rigs?: { rigId: string; nodes: Record<string, number> }[];
  id: string;
  name: string;
  fileName: string;
  relPath: string;
  pack: string;
  category: string;
  user: { favorite: boolean; tags: string[]; collections?: string[]; previewAssetId?: string; provenance?: ImportProvenance };
  variants?: AssetView[];
}

export interface ModelAssetView extends AssetViewBase {
  kind: "model";
  fileset: AssetFileset;
  animation: { clipCount: number; clipNames: string[] };
}

export interface MaterialAssetView extends AssetViewBase {
  kind: "material";
  material: { maps: MaterialMapInput[]; flipNormalGreen: boolean };
}

export interface AnimationAssetView extends AssetViewBase {
  kind: "animation";
  fileset: AssetFileset;
  clip: { standalone?: boolean; tracks?: {node: number; path: string}[]; index: number; name: string; duration: number; sourceId: string; sourceName: string; targetNodes: string[]; rig: string };
}
export interface AudioAssetView extends AssetViewBase {
  kind: "audio";
  audio: { file: string; format: string; duration: number; sampleRate: number; channels: number; bitDepth: number | null; byteLength: number; modifiedNs: string; peaks: number[] };
}
export type AssetView = ModelAssetView | MaterialAssetView | AnimationAssetView | AudioAssetView;

/** Expand only detail/size variants, preserving the primary asset's identity. */
export function exportVariants(assets: AssetView[], policy: unknown): AssetView[] {
  return assets.flatMap((a) => policy === "best" ? [a] : [a, ...(a.variants ?? [])]);
}

export interface AssetFileset {
  gltf: string;
  bin: string;
  textures: string[];
}

/** The result of an export/import run — mirrors the Rust `ExportReport`. */
export interface ExportReport {
  written: string[];
  skipped: string[];
  warnings: string[];
}

// --- manifest ----------------------------------------------------------------

export type PluginKind = "exporter" | "importer" | "rigAdapter";

/** Library types supported by the host, importers and exporters. */
export type AssetKind = "model" | "material" | "animation" | "audio";

/** What a plugin folder's `manifest.json` declares. */
export interface PluginManifest {
  /** Reverse-dns id; must match the plugin's folder name. */
  id: string;
  name: string;
  version: string;
  kind: PluginKind;
  /** Entry module, relative to the plugin dir (e.g. `index.js`). */
  entry: string;
  description?: string;
  /** Plugin-relative icon image; optional per-type overrides for navigation. */
  icon?: string;
  assetIcons?: Partial<Record<AssetKind,string>>;
  /** The asset kinds this plugin works with (an importer: what it produces).
   *  Absent means `["model"]`. */
  assetKinds?: AssetKind[];
  permissions: PluginPermissions;
  /** Config inputs the host renders before a run (the zero-code path). */
  fields?: FieldSpec[];
  /** Plugin-shipped React UI modules for host slots (the rich path). When a slot
   *  module is present the host mounts it instead of rendering `fields`. */
  ui?: PluginUi;
}

/** Relative paths (under the plugin dir) to the plugin's slot UI modules. Each
 *  default-exports a React component receiving the slot's typed `ctx` prop. */
export interface PluginUi {
  exportPanel?: string;
  importPanel?: string;
}

/**
 * The host enforces these by handing the plugin only the permitted slices of
 * `ctx`. Un-permitted methods are replaced with stubs that throw; the real
 * filesystem boundary is the host's Rust path jail.
 */
export interface PluginPermissions {
  /** May call `ctx.fs.write*` (writes are jailed to the run's target). */
  fsWrite?: boolean;
  /** May call `ctx.fs.read*`. */
  fsRead?: boolean;
  /** May call the Rust export primitives on `ctx.host`. */
  rustExport?: boolean;
  /** May call `ctx.host.convertToGltf` (shells the user's FBX2glTF). */
  rustConvert?: boolean;
}

/** One declarative config input. The host maps each to a ds primitive. */
export type FieldSpec =
  | { key: string; type: "text"; label: string; default?: string; placeholder?: string; hint?: string; required?: boolean }
  | { key: string; type: "checkbox"; label: string; default?: boolean }
  | { key: string; type: "select"; label: string; options: SelectOption[]; default?: string }
  | { key: string; type: "directory"; label: string; hint?: string; required?: boolean }
  | { key: string; type: "saveFile"; label: string; defaultName?: string; hint?: string; required?: boolean };

export interface SelectOption {
  value: string;
  label: string;
  desc?: string;
}

/** Collected field values, keyed by `FieldSpec.key`, plus the host-level shared
 *  inputs every mode gets. */
export type PluginConfig = Record<string, unknown> & {
  targetDir: string;
  preserveStructure: boolean;
};

// --- run-time host API (ctx.host / ctx.fs) -----------------------------------

/** Accumulates an `ExportReport` as a plugin runs. */
export interface ReportBuilder {
  write(path: string): void;
  skip(message: string): void;
  warn(message: string): void;
  done(): ExportReport;
}

/** Reads + Rust export primitives. The primitive half is gated by
 *  `permissions.rustExport`. */
export interface HostApi {
  previewExport(assetIds: string[], format: "glb" | "copy", preserveStructure: boolean): Promise<string[]>;
  getSelectedAssets(): AssetView[];
  getAsset(id: string): AssetView | undefined;
  getParsedGltf(id: string): Promise<unknown>;
  getAssetUrls(id: string): Promise<{ gltf: string; bin: string; textures: string[] }>;
  assembleGlb(id: string): Promise<Uint8Array>;
  performCopy(id: string, targetDir: string, stem: string, preserveStructure: boolean): Promise<string[]>;
  placerMerge(libraryJsonPath: string, subDirRes: string, assets: PlacerAssetInput[]): Promise<void>;
  transcodeImage(fileName: string, bytes: Uint8Array): Promise<{ mime: string; bytes: Uint8Array }>;
}

/** One asset to publish into a Godot `asset_library.json`. */
export interface PlacerAssetInput {
  pack: string;
  category: string;
  favorite: boolean;
  tags: string[];
  resPath: string;
  name: string;
}

/** One entry of a source-directory walk. `relPath` is relative to the
 *  `sourceRoot` it was read under, so a plugin recurses by passing it back. */
export interface DirEntry {
  name: string;
  relPath: string;
  isDir: boolean;
}

/** Jailed filesystem escape hatch. Reads are jailed to a caller-supplied
 *  `sourceRoot`, writes to a caller-supplied `authorizedRoot`; both reject any
 *  path that escapes their root. Reads gated by `permissions.fsRead`, writes by
 *  `permissions.fsWrite`. */
export interface FsApi {
  readDir(sourceRoot: string, path: string): Promise<DirEntry[]>;
  readBytes(sourceRoot: string, path: string): Promise<Uint8Array>;
  /** PNG bytes of the image at `path`, scaled down to fit `maxSize` × `maxSize`
   *  (aspect kept, never enlarged). Cheaper than `readBytes` for previews.
   *  Gated by `fsRead`. */
  readImageThumb(sourceRoot: string, path: string, maxSize: number): Promise<Uint8Array>;
  writeBytes(authorizedRoot: string, path: string, bytes: Uint8Array): Promise<void>;
  writeText(authorizedRoot: string, path: string, text: string): Promise<void>;
  writeJson(authorizedRoot: string, path: string, value: unknown): Promise<void>;
  pickDirectory(): Promise<string | null>;
  pickSaveFile(defaultName: string): Promise<string | null>;
}

/** The context an exporter plugin's `run` receives. */
export interface ExportCtx {
  host: HostApi;
  fs: FsApi;
  config: PluginConfig;
  report: ReportBuilder;
  signal: AbortSignal;
}

export interface ExporterPlugin {
  /** Pure planning step. Paths are relative to the chosen target folder. */
  preview?(ctx: ExportCtx, assets: AssetView[]): Promise<string[]>;
  run(ctx: ExportCtx, assets: AssetView[]): Promise<ExportReport>;
}

/** Importer plugins are panel-only — their work lives in the `importPanel` slot
 *  component, which builds `SeedEntryInput[]` and calls `ctx.commit`. */
export interface ImporterPlugin {
  readonly kind?: "importer";
}

export type MapRole =
  | "albedo"
  | "normal"
  | "roughness"
  | "metallic"
  | "ao"
  | "height"
  | "emissive"
  | "opacity";
export const MATERIAL_ROLE_LABELS: Record<MapRole, string> = {
  albedo: "Colour", normal: "Normal", roughness: "Roughness", metallic: "Metal",
  ao: "Shadowing", height: "Height", emissive: "Glow", opacity: "Opacity",
};
export type MapChannel = "r" | "g" | "b" | "a";

/** One texture map of a material set: a role, the image file, and optionally the
 *  channel of that file the scalar lives in. `invert` marks a scalar stored
 *  inverted (smoothness in place of roughness). */
export interface MaterialMapInput {
  role: MapRole;
  file: string;
  channel?: MapChannel;
  invert?: boolean;
}

export interface ModelSeedEntry {
  provenance?: ImportProvenance;
  variants?: ModelSeedEntry[];
  kind?: "model";
  id: string;
  pack: string;
  category: string;
  file: string;
}

export interface MaterialSeedEntry {
  provenance?: ImportProvenance;
  variants?: MaterialSeedEntry[];
  kind: "material";
  id: string;
  pack: string;
  category: string;
  name: string;
  maps: MaterialMapInput[];
  flipNormalGreen?: boolean;
}

/** What an importer produces — exactly the shape of a library manifest entry. */
export interface AnimationSeedEntry {
  provenance?: ImportProvenance;
  kind: "animation";
  id: string;
  pack: string;
  category: string;
  file: string;
}
export interface AudioSeedEntry { provenance?: ImportProvenance; kind: "audio"; id: string; pack: string; category: string; file: string; }
export type SeedEntryInput = ModelSeedEntry | MaterialSeedEntry | AnimationSeedEntry | AudioSeedEntry;

// --- slot UI (plugin-shipped React panels) -----------------------------------

/** Props every slot component receives: its typed render-time context. */
export interface SlotComponentProps<Ctx> {
  ctx: Ctx;
}

/** The render-time read+picker surface a slot component gets — narrower than the
 *  run-time `HostApi`/`FsApi` (gated write primitives fire at run/commit). */
export interface SlotHost {
  getSelectedAssets(): AssetView[];
  getAsset(id: string): AssetView | undefined;
  /** The configured library root, or null if none — where an importer writes. */
  getLibraryRoot(): Promise<string | null>;
  /** Convert one `.fbx` into loose glTF under `library/<pack>/<category>/`,
   *  returning the library-relative paths written (`.gltf` first). Shells the
   *  user's FBX2glTF; rejects if none is configured. Gated by `rustConvert`. */
  convertToGltf(
    srcPath: string,
    pack: string,
    category: string,
    stem: string,
  ): Promise<string[]>;
  pickDirectory(): Promise<string | null>;
  pickSaveFile(defaultName: string): Promise<string | null>;
}

/** Context for an exporter's `exportPanel`. */
export interface ExportPanelCtx {
  host: SlotHost;
  /** Saved values to restore when mounting a saved export. */
  initialConfig?: Record<string, unknown>;
  shared: { targetDir: string | null; preserveStructure: boolean };
  setConfig(values: Record<string, unknown>, ready: boolean): void;
}

/** Props of the host's interactive 3D material preview (sphere / plane / cube, orbit and zoom, tiling). */
export interface MaterialPreviewProps {
  controls?: ReactNode;
  maps: MaterialMapInput[];
  flipNormalGreen?: boolean;
  /** The URL to fetch a map's `file` from (e.g. an object URL of a source image). */
  resolveUrl(file: string): Promise<string>;
  /** Names the place the files come from. The preview reloads when it changes,
   *  so two sources with the same file names do not share a material. */
  sourceKey?: string;
}

/** Host-built components a panel can mount. */
export interface PanelWidgets {
  MaterialPreview: ComponentType<MaterialPreviewProps>;
}

/** Context for an importer's `importPanel`. Unlike an exporter (whose fs fires
 *  at run time), an importer does its file work during the panel session —
 *  walking a picked source and writing into the library before `commit` — so it
 *  carries the jailed `fs` directly. `widgets` are host-built components the
 *  panel mounts in its own tree, such as the 3D material preview. */
export interface ImportPanelCtx {
  /** The current library type, supplied by the host. */
  kind?: AssetKind;
  /** Optional host-owned header actions; clear them when a panel unmounts. */
  setActions?(actions: ReactNode): void;
  host: SlotHost;
  fs: FsApi;
  widgets: PanelWidgets;
  commit(entries: SeedEntryInput[]): Promise<void>;
  close(): void;
}

/** A plugin's export-drawer panel component. */
export type ExportPanelComponent = ComponentType<SlotComponentProps<ExportPanelCtx>>;
/** A plugin's import-drawer panel component. */
export type ImportPanelComponent = ComponentType<SlotComponentProps<ImportPanelCtx>>;

// --- authoring helpers (identity-typed sugar) --------------------------------

export function defineExporter(plugin: ExporterPlugin): ExporterPlugin {
  return plugin;
}

export function defineExportPanel(component: ExportPanelComponent): ExportPanelComponent {
  return component;
}

export function defineImportPanel(component: ImportPanelComponent): ImportPanelComponent {
  return component;
}

/** Build a fresh `ReportBuilder` that accumulates an `ExportReport`. Used by the
 *  host to seed `ctx.report`, and available to plugins assembling a report. */
export function createReportBuilder(): ReportBuilder {
  const report: ExportReport = { written: [], skipped: [], warnings: [] };
  return {
    write: (path) => report.written.push(path),
    skip: (message) => report.skipped.push(message),
    warn: (message) => report.warnings.push(message),
    done: () => report,
  };
}
