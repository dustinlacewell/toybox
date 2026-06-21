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

import type { ComponentType } from "react";

// --- catalog view ------------------------------------------------------------

/** The read-only view of a catalog asset a plugin sees. A self-contained mirror
 *  of the host's `Asset` (minus host-only fields like thumbnail state); the host
 *  passes assets typed as this. */
export interface AssetView {
  id: string;
  name: string;
  fileName: string;
  relPath: string;
  pack: string;
  category: string;
  fileset: AssetFileset;
  user: { favorite: boolean; tags: string[] };
  animation: { clipCount: number; clipNames: string[] };
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

export type PluginKind = "exporter" | "importer";

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

/** Jailed filesystem escape hatch. Gated by `permissions.fsRead`/`fsWrite`. */
export interface FsApi {
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
  run(ctx: ExportCtx, assets: AssetView[]): Promise<ExportReport>;
}

/** Importer plugins are panel-only — their work lives in the `importPanel` slot
 *  component, which builds `SeedEntryInput[]` and calls `ctx.commit`. */
export interface ImporterPlugin {
  readonly kind?: "importer";
}

/** What an importer produces — mirrors the Rust seed-entry shape. */
export interface SeedEntryInput {
  id: string;
  pack: string;
  category: string;
  file: string;
}

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
  pickDirectory(): Promise<string | null>;
  pickSaveFile(defaultName: string): Promise<string | null>;
}

/** Context for an exporter's `exportPanel`. */
export interface ExportPanelCtx {
  host: SlotHost;
  shared: { targetDir: string | null; preserveStructure: boolean };
  setConfig(values: Record<string, unknown>, ready: boolean): void;
}

/** Context for an importer's `importPanel`. */
export interface ImportPanelCtx {
  host: SlotHost;
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
