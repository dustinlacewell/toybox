/**
 * Catalog types, mirroring the Rust `catalog_model`. Pure data — no React,
 * three, or tauri imports. The primary key is the Godot `uid://` (basenames
 * collide across packs).
 */

import type { MaterialSet } from "./material";
import type { ImportProvenance } from "@ldlework/toybox-sdk";

/** Legacy source identifier retained for plugin/export compatibility.
 * Storage locations come from explicit file sets, never source/category names. */
export type Pack = string;

export interface PackMeta {
  slug: string;
  name: string;
  color: string;
}

export type ModelCategory =
  | "buildings"
  | "characters"
  | "environment"
  | "props"
  | "vehicles"
  | "weapons"
  | "fx"
  | "icons"
  | "primitives"
  | "roads"
  | "signs";

/** Material categories are free strings, so an asset's category is too. */
export type Category = string;

export type ThumbState = "missing" | "queued" | "rendering" | "ready" | "error";

export interface AssetFileset {
  /** Path to the .gltf, relative to libraryRoot. */
  gltf: string;
  bin: string;
  /** Unique texture paths, relative to libraryRoot. */
  textures: string[];
}

export interface ThumbMeta {
  state: ThumbState;
  error?: string;
  /** Runtime display metadata; the native cache remains the source of truth. */
  available?: boolean;
  revision?: number;
}

export interface UserMeta {
  provenance?: ImportProvenance;
  previewAssetId?: string;
  favorite: boolean;
  tags: string[];
  collections?: string[];
}

export interface AnimationMeta {
  clipCount: number;
  clipNames: string[];
}

interface AssetBase {
  /** The library the asset lives in. Stamped when the catalog arrives from the backend; `id` is then `<library>::<raw id>` (see `libraryId.ts`). */
  library: string;
  rigs?: RigBinding[];
  /** Alternate file sets of this same thing, ordered best first after the primary. */
  variants?: Asset[];
  id: string;
  name: string;
  fileName: string;
  /** The .gltf for a model; the folder for a material. Relative to libraryRoot. */
  relPath: string;
  pack: Pack;
  category: Category;
  thumb: ThumbMeta;
  user: UserMeta;
  /** Tags: descriptive keywords ("footstep", "carpet") stored in the library where the file is, so everyone
   *  who has the library sees them. Lower-case. `user.tags` holds only the tags derived from provenance
   *  (and tags an older version kept on this computer until they are moved). Absent when there are none.
   *  Search and the Tags filter use both. */
  tags?: string[];
  /** The plugin that imported the asset and its own record of how. */
  origin?: { plugin: string; meta?: Record<string, unknown>; files?: string[]; derivedFrom?: string };
  /** The file format it was imported from ("fbx", "obj", ...): lower-case, no dot. Absent on older imports. */
  format?: string;
  /** Fingerprint of the files (hex SHA-256), the same for byte-identical content whatever the id,
   *  name or pack. Derived by the backend (side cache, or the server's); absent until computed.
   *  How a copy in another library is recognised (see `domain/duplicates`). */
  contentHash?: string;
}

export interface ModelAsset extends AssetBase {
  kind: "model";
  fileset: AssetFileset;
  animation: AnimationMeta;
}

export interface MaterialAsset extends AssetBase {
  kind: "material";
  material: MaterialSet;
}

export interface AnimationClipMeta {
  standalone?: boolean;
  tracks?: { node: number; path: string }[];
  index: number;
  name: string;
  duration: number;
  sourceId: string;
  sourceName: string;
  targetNodes: string[];
  rig: string;
}

export interface AnimationAsset extends AssetBase {
  kind: "animation";
  fileset: AssetFileset;
  clip: AnimationClipMeta;
}

export type SceneAsset = ModelAsset | AnimationAsset;

export interface AudioAsset extends AssetBase {
  kind: "audio";
  audio: { file: string; format: string; duration: number; sampleRate: number; channels: number; bitDepth: number | null; byteLength: number; modifiedNs: string; peaks: number[] };
}
/** An asset of a plugin-defined type: a file set plus the type's own metadata.
 *  `files` are library-relative; the host never interprets `meta`. */
export interface PluginAsset extends AssetBase {
  kind: "plugin";
  type: string;
  files: string[];
  meta: Record<string, unknown>;
}
export type Asset = ModelAsset | MaterialAsset | AnimationAsset | AudioAsset | PluginAsset;

/** The kinds the host implements natively. */
export type BuiltinKind = "model" | "material" | "animation" | "audio";
/** A library type id: a built-in kind or a plugin-declared type id. */
export type AssetKind = BuiltinKind | (string & {});

/** The library type an asset is listed under. */
export const assetType = (a: Asset): AssetKind => (a.kind === "plugin" ? a.type : a.kind);

export const isModel = (a: Asset): a is ModelAsset => a.kind === "model";
export const isSceneAsset = (a: Asset): a is SceneAsset => a.kind === "model" || a.kind === "animation";

/** The merged catalog the app holds: the assets of every library, each stamped
 *  with its `library`, ids qualified. There is no single library root. */
export interface Catalog {
  rigs?: RigDefinition[];
  schemaVersion: number;
  assets: Asset[];
}

/** One library's catalog exactly as the backend reads and writes it: raw ids,
 *  assets without a `library` stamp. Only `services/tauriApi` and
 *  `libraryId` handle this form. */
export interface LibraryCatalog extends Catalog {
  libraryRoot: string;
}

/** A registered library, as the backend reports it. */
export interface LibraryInfo {
  id: string;
  name: string;
  kind: "local" | "remote";
  /** The folder of a local library; for a remote one the local folder its
   *  downloaded files (and an import being prepared) live in. */
  root: string;
  /** False while the library cannot be read (an unplugged folder; a remote
   *  library that was never reachable and has nothing cached). */
  available: boolean;
  /** Whether items may be imported into, deleted from or transferred into this
   *  library. False for a read-only folder or token, an unavailable library, or
   *  a remote one that is offline. */
  canWrite: boolean;
  /** Whether the library's files may be edited in place (origin alignment).
   *  Never true for a remote library. Absent in older answers: same as `canWrite`. */
  canEditFiles?: boolean;
  isDefault: boolean;
  /** A remote library shown from its cache because the server cannot be reached. */
  offline?: boolean;
  offlineReason?: string;
  /** Present for a remote library. */
  remote?: RemoteInfo;
}

/** What is known about a remote library's server and the token used with it. */
export interface RemoteInfo {
  url: string;
  role: "read" | "write" | "admin";
  serverName: string;
  offline: boolean;
  /** The server's Toybox version, when it has said. */
  serverVersion?: string;
  /** The oldest Toybox the server serves. */
  minClientVersion?: string;
  /** How the server's version fits this app: ok, notice, server_too_old or client_too_old. */
  compat?: "ok" | "notice" | "server_too_old" | "client_too_old";
  /** The sentence to show about the versions, when there is one. */
  compatMessage?: string;
}

export interface RigBinding { rigId: string; nodes: Record<string, number>; }
export interface RigDefinition {
  id: string;
  name: string;
  kind: "skeleton" | "nodes";
  unambiguous: boolean;
  nodes: { key: string; name: string; parent: string | null; rest: number[] }[];
}
