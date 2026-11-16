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
export type Asset = ModelAsset | MaterialAsset | AnimationAsset | AudioAsset;

export type AssetKind = Asset["kind"];

export const isModel = (a: Asset): a is ModelAsset => a.kind === "model";
export const isSceneAsset = (a: Asset): a is SceneAsset => a.kind === "model" || a.kind === "animation";
export const ASSET_KIND_LABELS: Record<AssetKind, string> = { model: "Models", material: "Materials", animation: "Animations", audio: "Audio" };

export interface Catalog {
  rigs?: RigDefinition[];
  schemaVersion: number;
  libraryRoot: string;
  assets: Asset[];
}

export interface RigBinding { rigId: string; nodes: Record<string, number>; }
export interface RigDefinition {
  id: string;
  name: string;
  kind: "skeleton" | "nodes";
  unambiguous: boolean;
  nodes: { key: string; name: string; parent: string | null; rest: number[] }[];
}
