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
export { defineRigAdapter, humanoidRig, type RigProfile, type RigAdapterPlugin, type HumanoidRigSpec, type HumanoidSide, type HumanoidFinger, type BoneNames } from "./rig.js";
import type { RigProfile } from "./rig.js";

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
  /** The library the asset lives in. Opaque: `id` is qualified with it, so ids are unique across libraries. */
  library?: string;
  rigs?: { rigId: string; nodes: Record<string, number> }[];
  id: string;
  name: string;
  fileName: string;
  relPath: string;
  pack: string;
  category: string;
  user: { favorite: boolean; tags: string[]; collections?: string[]; previewAssetId?: string; provenance?: ImportProvenance };
  variants?: AssetView[];
  /** The plugin that imported the asset, and what it recorded about how. */
  origin?: AssetOrigin;
  /** The file format it was imported from: short, lower-case, no dot ("fbx", "obj", "glb", "sbsar", "wav"). Absent on assets imported before it was recorded. */
  format?: string;
}

/** Where an asset came from inside Toybox. The host stamps `plugin` with the
 *  importing plugin's id; `meta` is that plugin's own record — for a generated
 *  material, say, the parameters it was rendered with; `files` are the source
 *  files the plugin keeps with the asset, as library-relative paths. The host
 *  stores `meta` and `files` and hands them back without interpreting them.
 *
 *  A kept source file is stored once per library, by content, in a place shared by
 *  every item that keeps the same file (`library/<type>/_sources/<sha256>/<name>`).
 *  A path in `files` does not lie in the item's own folder, and its last segment is
 *  the file's original name: find a file by that name (or by `meta`), never by where
 *  it sits. The path is stable while any item keeps the file. */
export interface AssetOrigin {
  plugin: string;
  meta?: Record<string, unknown>;
  files?: string[];
}

/** What an importer may record on an entry it commits. The host fills in
 *  `plugin` with the importing plugin's id; an importer supplies `meta` and
 *  `files`: bare names of source files staged next to the entry's own files (a
 *  material's maps, a model's glTF), such as the archive a material was
 *  generated from or the `.fbx` a model was converted from. At commit the host
 *  moves each into shared, content-addressed storage (one copy per distinct file
 *  in the library, whatever number of items keep it; a staged copy of a file that
 *  is stored already is just dropped) and records its library-relative path in
 *  `asset.origin.files`. A source travels with the item, is deleted with the last
 *  item that keeps it, and is not exported by Copy files. Materials and models can
 *  keep them. A model's list must not reuse the names `<stem>.gltf` and `<stem>.bin`.
 *
 *  An importer may name another plugin in `plugin` when that plugin is on and
 *  directly depends on the importer (a format plugin of `model-core`): the
 *  host keeps the name only then, and stamps its own id otherwise. */
export interface SeedOrigin {
  plugin?: string;
  meta?: Record<string, unknown>;
  files?: string[];
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
/** An asset of a plugin-defined type. The host stores, indexes and lists it
 *  generically; `type` names the declaring `AssetTypeDeclaration`, `files` are
 *  library-relative paths of its file set, and `meta` is whatever the type's
 *  importer recorded (the host never interprets it). */
export interface PluginAssetView extends AssetViewBase {
  kind: "plugin";
  type: string;
  files: string[];
  meta: Record<string, unknown>;
}
export type AssetView = ModelAssetView | MaterialAssetView | AnimationAssetView | AudioAssetView | PluginAssetView;

/** The library type an asset is listed under: a plugin asset's declared type,
 *  otherwise its built-in kind. */
export function assetTypeOf(asset: { kind: string; type?: string }): AssetKind {
  return asset.kind === "plugin" && asset.type ? asset.type : asset.kind;
}

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

/** @deprecated Plugins no longer have a kind. What a plugin does follows from
 *  what it ships: `assetTypes` declare library types, `ui.importPanel` makes it
 *  an importer, and its entry module (`PluginModule`) may export, carry rig
 *  profiles, implement types and contribute to extension points, in any mix. A
 *  `kind` in an older manifest is ignored. */
export type PluginKind = "exporter" | "importer" | "rigAdapter" | "assetType";

/** The types the host implements natively. */
export type BuiltinAssetKind = "model" | "material" | "animation" | "audio";

/** A library type id: a built-in kind or the id of a plugin-declared type. */
export type AssetKind = BuiltinAssetKind | (string & {});

/** Type ids are lower-case words; they also name the type's storage folder. */
export const ASSET_TYPE_ID = /^[a-z][a-z0-9-]{1,31}$/;

/** One library type a plugin declares in its manifest (`assetTypes`). The type
 *  exists in the library because a plugin declares it. */
export interface AssetTypeDeclaration {
  /** Stable id, e.g. `skybox`. Matches `ASSET_TYPE_ID`. */
  id: string;
  /** Plural navigation label, e.g. `Skyboxes`. */
  label: string;
  /** Singular noun, e.g. `Skybox`. Defaults to `label`. */
  singular?: string;
  /** Plugin-relative navigation icon. Falls back to the plugin `icon`. */
  icon?: string;
  /** `plugin` (default): the plugin's entry module implements the type and its
   *  assets use the generic `PluginAssetView` shape. `native`: the host
   *  implements viewing and indexing; the id must be a built-in kind. */
  implementation?: "plugin" | "native";
  /** Width / height of the type's library tiles. Defaults to 1. */
  tileAspectRatio?: number;
  /** Position in the navigation; lower first. Defaults to 100. */
  order?: number;
}

/** What a plugin folder's `manifest.json` declares. */
export interface PluginManifest {
  /** Reverse-dns id; must match the plugin's folder name. */
  id: string;
  name: string;
  version: string;
  /** @deprecated Ignored. See `PluginKind`. */
  kind?: PluginKind;
  /** Entry module, relative to the plugin dir (e.g. `dist/index.js`). Its
   *  default export is a `PluginModule`, or a function of `PluginCtx` returning
   *  one (`definePlugin`). Optional: a plugin can consist of panels alone. */
  entry?: string;
  /** Plugins this one needs, by id, each with the versions it accepts as a
   *  semver range (`"^1.0.0"`, or `"*"` for any). Turning this plugin on turns
   *  them on; they cannot be turned off while this plugin is on; and this
   *  plugin stays unloaded while one is missing, off or too old. */
  dependencies?: Record<string, string>;
  description?: string;
  /** Plugin-relative icon image; optional per-type overrides for navigation. */
  icon?: string;
  assetIcons?: Partial<Record<AssetKind,string>>;
  /** The library types this plugin's importer produces and its exporter
   *  accepts. Absent means the types it declares, or `["model"]` when it
   *  declares none. `"*"` is every type. */
  assetKinds?: AssetKind[];
  /** The library types this plugin defines. */
  assetTypes?: AssetTypeDeclaration[];
  permissions: PluginPermissions;
  /** Config inputs the host renders before a run (the zero-code path). */
  fields?: FieldSpec[];
  /** Settings the user fills in once, under the plugin in Settings → Plugins:
   *  the location of an external program, a default quality, and so on. Read
   *  them with `ctx.services.settings`. */
  settings?: SettingSpec[];
  /** Plugin-shipped React UI modules for host slots (the rich path). When a slot
   *  module is present the host mounts it instead of rendering `fields`. */
  ui?: PluginUi;
  /** What the plugin adds to the `tbx` command line: importers, an exporter
   *  and commands, declared here so help and validation work without running
   *  plugin code, and implemented in `cli.module`. */
  cli?: CliSpec;
}

// --- the command line (`tbx`) --------------------------------------------------

/** The value types of a command-line argument or option. A relative `path` is
 *  made absolute against the current folder before the plugin sees it. */
export type CliValueType = "string" | "number" | "flag" | "choice" | "path";

/** The `cli` section of a manifest. */
export interface CliSpec {
  /** Plugin-relative ES module with **no user interface** (it must not import
   *  React, three.js or anything that needs a window). Its default export is a
   *  `PluginModule`, or a function of `PluginCtx` returning one, and carries
   *  the handlers under `cli`. A plugin whose `entry` is already like that
   *  names the same file here. `tbx` loads this module of every active plugin
   *  and never `entry` or the panels. */
  module: string;
  /** Becomes `tbx import <name>`; handled by `PluginModule.cli.importers[name]`. */
  importers?: CliImporterSpec[];
  /** Becomes `tbx export <name>`; handled by the module's `run` and `preview`,
   *  exactly as in the app. */
  exporter?: CliExporterSpec;
  /** Top-level commands, `tbx <name>`; handled by `PluginModule.cli.commands`. */
  commands?: CliCommandSpec[];
}

/** One option of an importer, exporter or command, given as `--name`. */
export interface CliOptionSpec {
  /** `kebab-case`. The plugin receives the value under the camelCase of it (`flip-green` → `flipGreen`). */
  name: string;
  type: CliValueType;
  /** The allowed values of a `choice`. */
  choices?: string[];
  /** Used when the option is not given. */
  default?: string | number | boolean;
  /** The command is refused without it. */
  required?: boolean;
  /** May be given several times; the plugin receives an array. */
  multiple?: boolean;
  /** One line. */
  help?: string;
}

/** One positional argument of a command. Same fields as an option, but a
 *  positional argument is never a `flag`. With `multiple` it takes the rest of
 *  the line. */
export interface CliArgSpec extends Omit<CliOptionSpec, "type"> {
  type: Exclude<CliValueType, "flag">;
}

/** An importer: `tbx import <name> <PATH>... [options]`. The paths and the
 *  shared options (`--tag`, `--source`, ...) are not declared here. */
export interface CliImporterSpec {
  /** `[a-z][a-z0-9-]*`. */
  name: string;
  summary: string;
  options?: CliOptionSpec[];
}

/** An exporter: `tbx export <name> <SELECTION> --to <DIR> [options]`. Besides the
 *  options, `config` carries `targetDir` (`--to`), `preserveStructure` and
 *  `variantPolicy` (`"all"` or `"best"`, from `--variants`), exactly as the app's
 *  export drawer supplies them. With no
 *  `options` they are taken from the manifest's `fields` (`text` → string,
 *  `checkbox` → flag, `select` → choice, `directory` and `saveFile` → path). */
export interface CliExporterSpec {
  /** `[a-z][a-z0-9-]*`. */
  name: string;
  summary: string;
  options?: CliOptionSpec[];
}

/** A command, either a group (`commands`) or runnable (`args`/`options`), never
 *  both. Groups nest; a command is handled under the names of its path joined
 *  with spaces (`"sbsar inputs"`). */
export interface CliCommandSpec {
  /** `[a-z][a-z0-9-]*`. */
  name: string;
  summary: string;
  /** Present: this is a group, and has no `args`, `options` or `selection`. */
  commands?: CliCommandSpec[];
  args?: CliArgSpec[];
  options?: CliOptionSpec[];
  /** The command accepts the standard item selection (ids or a filter) and
   *  receives the selected items as `CommandRun.assets`. */
  selection?: boolean;
}

/** A parsed option or argument value: a string or `path` (absolute), a number,
 *  a flag, or a list of these for `multiple`. */
export type CliValue = string | number | boolean | string[] | number[];

/** What both kinds of run have in common. `host`, `fs` and `services` are the
 *  app's own interfaces with the plugin's permissions; there are no dialogs
 *  (`pickDirectory` and `pickSaveFile` reject). */
export interface CliRunBase {
  /** The library the command acts on. */
  library: { id: string; name: string };
  host: SlotHost;
  fs: FsApi;
  services: PluginServices;
  extensions: ExtensionLookup;
  /** Stage-then-commit exactly as an import panel does. */
  commit(entries: SeedEntryInput[]): Promise<void>;
  /** Progress, on standard error. */
  log(message: string): void;
  /** A warning, on standard error. */
  warn(message: string): void;
  signal: AbortSignal;
}

/** What an importer's handler receives. */
export interface ImportRun extends CliRunBase {
  /** The files and folders given on the command line, absolute. */
  paths: string[];
  /** The importer's options by camelCase name; defaults applied. */
  options: Record<string, CliValue | undefined>;
}

/** What a command's handler receives. */
export interface CommandRun extends CliRunBase {
  args: Record<string, CliValue | undefined>;
  options: Record<string, CliValue | undefined>;
  /** The selected items, for a command declared with `selection`. */
  assets?: AssetView[];
  /** Standard output. A command that prints nothing has its return value printed instead. */
  print(text: string): void;
}

/** An item an import replaced: its entry had the id of an item the library already had. */
export interface ReplacedItem {
  /** The id of the item, as in `ImportResult.imported`. */
  id: string;
  name: string;
  /** In a few words, what the replaced item was (for a generated material: how it was rendered). */
  was?: string;
}

/** What an importer reports. The ids are those of the committed entries. */
export interface ImportResult {
  imported?: string[];
  /** Items the import replaced rather than added. `tbx` says so, one line each, and lists them in its JSON. */
  replaced?: ReplacedItem[];
  /** Why each file or item was left out. */
  skipped?: string[];
  warnings?: string[];
}

/** Relative paths (under the plugin dir) to the plugin's slot UI modules. Each
 *  default-exports a React component receiving the slot's typed `ctx` prop. */
export interface PluginUi {
  exportPanel?: string;
  importPanel?: string;
  /** A panel shown in the viewer of every asset this plugin imported
   *  (`asset.origin.plugin`), for example to change how it was generated. */
  assetPanel?: string;
  /** Tab label of `assetPanel`. Defaults to the plugin's name. */
  assetPanelLabel?: string;
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
  /** May start a program one of its `file` settings points at
   *  (`ctx.services.tools`). The user chooses the program, never the plugin. */
  runTools?: boolean;
  /** May exchange messages with services on this machine
   *  (`ctx.services.local`; `127.0.0.1` only). */
  localNetwork?: boolean;
}

/** One setting a plugin declares. `file` and `directory` are paths the user
 *  picks; a text `default` may use `{appData}`, `{localAppData}` and `{home}`
 *  to name a program's usual location on any machine. */
export type SettingSpec =
  | { key: string; type: "text"; label: string; default?: string; placeholder?: string; hint?: string }
  | { key: string; type: "checkbox"; label: string; default?: boolean; hint?: string }
  | { key: string; type: "select"; label: string; options: SelectOption[]; default?: string; hint?: string }
  | { key: string; type: "directory"; label: string; default?: string; hint?: string }
  | { key: string; type: "file"; label: string; default?: string; hint?: string };

/** Host services for a plugin that drives an external program, so all of that
 *  work can live in the plugin. Each is gated by the manifest's permissions. */
export interface PluginServices {
  settings: {
    /** The plugin's settings: what the user chose, else each declared default. */
    get(): Promise<Record<string, unknown>>;
    /** Store one declared setting; `null` returns it to its default. */
    set(key: string, value: unknown): Promise<void>;
  };
  tools: {
    /** Start the program the `file` setting `setting` points at, with `args`,
     *  in the program's own folder. It runs in the background and is stopped
     *  when Toybox closes. Needs `runTools`. */
    launch(setting: string, args: string[]): Promise<void>;
    /** Run the program the `file` setting `setting` points at, with `args`, in
     *  the program's own folder, and wait for it to finish (a converter, a
     *  headless render). Rejects when it cannot start or outlives `timeoutMs`
     *  (default two minutes); a non-zero exit code is returned, not thrown.
     *  Needs `runTools`. */
    run(setting: string, args: string[], options?: { timeoutMs?: number }): Promise<ToolResult>;
    /** Toybox's own process id, for a program that tracks who it works for. */
    hostProcessId(): Promise<number>;
  };
  local: {
    /** Whether something on this machine listens on `port`. Needs `localNetwork`. */
    isOpen(port: number): Promise<boolean>;
    /** Send one JSON message to `127.0.0.1:port` and return the JSON reply.
     *  Needs `localNetwork`. */
    requestJson(port: number, message: unknown, timeoutMs?: number): Promise<unknown>;
  };
  /** Absolute path of the plugin's private folder for intermediate files.
   *  `clear` empties it first. Nothing in it is kept. */
  scratchDir(clear?: boolean): Promise<string>;
}

/** How a program run with `services.tools.run` ended. */
export interface ToolResult {
  code: number;
  stdout: string;
  stderr: string;
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
  /** `getSelectedAssets` for a selection the host has not loaded: the items are fetched by id first. The host does not
   *  hold the library's catalog; `getSelectedAssets` and `getAsset` answer from the items it has loaded, which includes
   *  the selection of a run. */
  getSelectedAssetsAsync(): Promise<AssetView[]>;
  /** `getAsset`, fetched by id when the host has not loaded the item. `undefined` for an id that is not in a library. */
  getAssetAsync(id: string): Promise<AssetView | undefined>;
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
  /** Copy one file natively, without passing its bytes through the plugin.
   *  The source is jailed to `sourceRoot`, the destination to `authorizedRoot`.
   *  Gated by `fsRead` and `fsWrite`. */
  copyFile(sourceRoot: string, path: string, authorizedRoot: string, target: string): Promise<void>;
  writeBytes(authorizedRoot: string, path: string, bytes: Uint8Array): Promise<void>;
  writeText(authorizedRoot: string, path: string, text: string): Promise<void>;
  writeJson(authorizedRoot: string, path: string, value: unknown): Promise<void>;
  /** Remove a file or folder (recursively) below `authorizedRoot`, jailed like the writes; a path that is not
   *  there is not an error and the root itself is refused. Gated by `fsWrite`. Only `tbx` provides it so far
   *  (to tidy up a plugin's scratch files); the app's `fs` has none, so call it as `fs.removeTree?.(...)`. */
  removeTree?(authorizedRoot: string, path: string): Promise<void>;
  pickDirectory(): Promise<string | null>;
  pickSaveFile(defaultName: string): Promise<string | null>;
}

/** The context an exporter plugin's `run` receives. */
export interface ExportCtx {
  host: HostApi;
  fs: FsApi;
  services: PluginServices;
  extensions: ExtensionLookup;
  config: PluginConfig;
  report: ReportBuilder;
  signal: AbortSignal;
}

export interface ExporterPlugin {
  /** Pure planning step. Paths are relative to the chosen target folder. */
  preview?(ctx: ExportCtx, assets: AssetView[]): Promise<string[]>;
  run(ctx: ExportCtx, assets: AssetView[]): Promise<ExportReport>;
}

/** @deprecated Importers have no object of their own. An importer is a plugin with `ui.importPanel` (the panel builds `SeedEntryInput[]` and calls `ctx.commit`) and, for the command line, `cli.importers` in its manifest answered by `PluginModule.cli.importers`. Both should call the same UI-free code. */
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

export { importNamespace } from "./storage.js";
export { resolveSources, type ImportSource } from "./sources.js";
export { validSegment, assertCategory, newImportOutcome, noteSkip, mergeImportOutcome, runImport, type ImportOutcome } from "./cliImport.js";

export interface ModelSeedEntry {
  /** Host-managed file location; source/category never define storage. */
  storagePath?: string;
  provenance?: ImportProvenance;
  /** Descriptive keywords that belong to the asset and live in the library itself, so everyone who has the library sees them ("footstep", "carpet", "loop"). Normalised by the host: trimmed, lower-cased, de-duplicated; a tag with a colon is dropped; at most 64 tags of 48 characters. Distinct from the personal tags a user keeps on their own computer. */
  tags?: string[];
  origin?: SeedOrigin;
  /** The file format it was imported from: short, lower-case, no dot ("fbx", "obj", "glb", "sbsar", "wav"). Drives the Format filter. */
  format?: string;
  variants?: ModelSeedEntry[];
  kind?: "model";
  id: string;
  pack: string;
  category: string;
  file: string;
}

export interface MaterialSeedEntry {
  storagePath?: string;
  provenance?: ImportProvenance;
  /** Descriptive keywords that belong to the asset and live in the library itself, so everyone who has the library sees them ("footstep", "carpet", "loop"). Normalised by the host: trimmed, lower-cased, de-duplicated; a tag with a colon is dropped; at most 64 tags of 48 characters. Distinct from the personal tags a user keeps on their own computer. */
  tags?: string[];
  origin?: SeedOrigin;
  /** The file format it was imported from: short, lower-case, no dot ("fbx", "obj", "glb", "sbsar", "wav"). Drives the Format filter. */
  format?: string;
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
  storagePath?: string;
  provenance?: ImportProvenance;
  /** Descriptive keywords that belong to the asset and live in the library itself, so everyone who has the library sees them ("footstep", "carpet", "loop"). Normalised by the host: trimmed, lower-cased, de-duplicated; a tag with a colon is dropped; at most 64 tags of 48 characters. Distinct from the personal tags a user keeps on their own computer. */
  tags?: string[];
  origin?: SeedOrigin;
  format?: string;
  kind: "animation";
  id: string;
  pack: string;
  category: string;
  file: string;
}
export interface AudioSeedEntry { storagePath?: string; provenance?: ImportProvenance; /** Library tags, as on the other seed entries. */ tags?: string[]; origin?: SeedOrigin; format?: string; kind: "audio"; id: string; pack: string; category: string; file: string; }
/** A plugin-defined asset: a named set of files plus the type's own metadata.
 *  Before commit the files live at `library/<pack>/<type>/<name>/<file>`; the
 *  host moves them into managed storage (`library/<type>/<key>/`). */
export interface PluginSeedEntry {
  storagePath?: string;
  provenance?: ImportProvenance;
  /** Descriptive keywords that belong to the asset and live in the library itself, so everyone who has the library sees them ("footstep", "carpet", "loop"). Normalised by the host: trimmed, lower-cased, de-duplicated; a tag with a colon is dropped; at most 64 tags of 48 characters. Distinct from the personal tags a user keeps on their own computer. */
  tags?: string[];
  origin?: SeedOrigin;
  format?: string;
  kind: "plugin";
  /** The declaring `AssetTypeDeclaration.id`. */
  type: string;
  id: string;
  pack: string;
  category: string;
  name: string;
  /** Bare file names of the set. */
  files: string[];
  meta?: Record<string, unknown>;
}
export type SeedEntryInput = ModelSeedEntry | MaterialSeedEntry | AnimationSeedEntry | AudioSeedEntry | PluginSeedEntry;

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
  /** `getSelectedAssets`, fetching the items by id first (see `HostApi.getSelectedAssetsAsync`). */
  getSelectedAssetsAsync(): Promise<AssetView[]>;
  /** `getAsset`, fetched by id when the host has not loaded the item. */
  getAssetAsync(id: string): Promise<AssetView | undefined>;
  /** The root of the library this panel works in, or null if none: the library an
   *  import was sent into (see `ImportPanelCtx.library`), or that of an asset
   *  panel's asset. Where an importer writes. */
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
  services: PluginServices;
  extensions: ExtensionLookup;
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
  /** The library this import goes into (the user's choice, the default library
   *  otherwise). `host.getLibraryRoot()` already points at its root, so an importer
   *  that only writes files needs nothing from here; it is for panels that want
   *  to show or remember where an import went. Absent in older hosts. */
  library?: { id: string; name: string };
  /** Optional host-owned header actions; clear them when a panel unmounts. */
  setActions?(actions: ReactNode): void;
  host: SlotHost;
  fs: FsApi;
  services: PluginServices;
  extensions: ExtensionLookup;
  widgets: PanelWidgets;
  commit(entries: SeedEntryInput[]): Promise<void>;
  close(): void;
}

/** Context of a plugin's `assetPanel`: one asset the plugin imported, shown in
 *  that asset's viewer. `commit` adds entries to the library exactly as an
 *  import does — an entry that reuses `asset.id` replaces that item. */
export interface AssetPanelCtx {
  asset: AssetView;
  host: SlotHost;
  fs: FsApi;
  services: PluginServices;
  extensions: ExtensionLookup;
  widgets: PanelWidgets;
  commit(entries: SeedEntryInput[]): Promise<void>;
}

/** A plugin's export-drawer panel component. */
export type ExportPanelComponent = ComponentType<SlotComponentProps<ExportPanelCtx>>;
/** A plugin's import-drawer panel component. */
export type ImportPanelComponent = ComponentType<SlotComponentProps<ImportPanelCtx>>;
/** A plugin's viewer panel for the assets it imported. */
export type AssetPanelComponent = ComponentType<SlotComponentProps<AssetPanelCtx>>;

// --- plugin-implemented asset types ------------------------------------------

/** File access for a plugin-defined asset, scoped to that asset's own files. */
export interface AssetFilesApi {
  /** A URL the webview can load `file` from (one of `asset.files`; defaults to
   *  the first). Usable as an image/media source and with `fetch`. */
  fileUrl(asset: PluginAssetView, file?: string): Promise<string>;
  /** The bytes of one of the asset's files. */
  readFile(asset: PluginAssetView, file?: string): Promise<Uint8Array>;
}

/** One row of the inspector's property list. */
export interface AssetProperty {
  label: string;
  value: string | number;
}

/** Context of a type's full-view `Viewer`. The host supplies the surrounding
 *  frame (title, details, favourite, export); the component fills the stage. */
export interface AssetViewerCtx {
  asset: PluginAssetView;
  files: AssetFilesApi;
  /** Controls for the frame's side panel; clear them when unmounting. */
  setSidebar(content: ReactNode): void;
  /** Controls for the frame's footer; clear them when unmounting. */
  setFooter(content: ReactNode): void;
}

/** Everything a plugin implements for one of its declared types. All members
 *  are optional: a type with none still imports, lists, filters and exports. */
export interface AssetTypeImplementation {
  /** Full-view stage for one asset. */
  Viewer?: ComponentType<SlotComponentProps<AssetViewerCtx>>;
  /** A square PNG thumbnail for the library grid. */
  renderThumbnail?(asset: PluginAssetView, files: AssetFilesApi, size: number): Promise<Uint8Array>;
  /** Type-specific rows of the inspector's property list. */
  properties?(asset: PluginAssetView): AssetProperty[];
  /** Second line of a library tile. Defaults to the source title. */
  subtitle?(asset: PluginAssetView): string;
}

/** What an `assetType` plugin's entry module default-exports: one
 *  implementation per plugin-implemented type it declares, keyed by type id. */
export interface AssetTypePlugin {
  types: Record<string, AssetTypeImplementation>;
}

export function defineAssetTypes(plugin: AssetTypePlugin): AssetTypePlugin {
  return plugin;
}

// --- the entry module, and extension points ----------------------------------

/** One contribution to an extension point, and the plugin it came from. */
export interface Extension<T = unknown> {
  pluginId: string;
  value: T;
}

/** Everything the enabled plugins contribute to the extension point `point`,
 *  ordered by plugin id. Looked up when called, so it is always current. */
export type ExtensionLookup = <T = unknown>(point: string) => Extension<T>[];

/** What a plugin's entry module receives when the host loads it: the plugin's
 *  own id and its permitted slices of the host. Anything the module hands out
 *  (an extension, say, that another plugin's panel later calls) keeps working
 *  with this plugin's own permissions and settings. */
export interface PluginCtx {
  id: string;
  host: SlotHost;
  fs: FsApi;
  services: PluginServices;
  extensions: ExtensionLookup;
}

/** What a plugin's entry module provides. Every member is optional and they
 *  combine freely: one plugin can export, implement a type and extend another. */
export interface PluginModule {
  /** Makes the plugin an exporter, listed for the types in its `assetKinds`. */
  run?(ctx: ExportCtx, assets: AssetView[]): Promise<ExportReport>;
  /** Exporter planning step: paths relative to the chosen target folder. */
  preview?(ctx: ExportCtx, assets: AssetView[]): Promise<string[]>;
  /** Rig profiles (see `defineRigAdapter`). */
  profiles?: RigProfile[];
  /** Implementations of the plugin-implemented types the manifest declares. */
  types?: Record<string, AssetTypeImplementation>;
  /** Contributions to extension points, keyed by point name. A point is a
   *  contract another plugin publishes: a core plugin names it (by convention
   *  `<its subject>.<what>`, e.g. `model.formats`) and documents the shape of a
   *  contribution; plugins that depend on it contribute here; the core plugin
   *  reads them with `ctx.extensions(point)`. The host only carries them. */
  extensions?: Record<string, unknown[]>;
  /** What this module answers on the command line, matching the manifest's
   *  `cli` section: an importer per `cli.importers[].name`, a command per
   *  runnable `cli.commands` path joined with spaces. Both sides must agree. */
  cli?: {
    importers?: Record<string, (run: ImportRun) => Promise<ImportResult | void>>;
    commands?: Record<string, (run: CommandRun) => Promise<unknown>>;
  };
}

/** Declare a plugin's entry module: either the module itself, or a function of
 *  the plugin's `PluginCtx` that builds it (use this when a contribution needs
 *  the plugin's own file access, settings or tools). */
export function definePlugin<T extends PluginModule | ((ctx: PluginCtx) => PluginModule | Promise<PluginModule>)>(plugin: T): T {
  return plugin;
}

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

export function defineAssetPanel(component: AssetPanelComponent): AssetPanelComponent {
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
