/**
 * Catalog types, mirroring the Rust `catalog_model`. Pure data — no React,
 * three, or tauri imports. The primary key is the Godot `uid://` (basenames
 * collide across packs).
 */

import type { MaterialSet } from "./material";

/**
 * A pack is the top-level division of the library (`library/<pack>/...`).
 * Identity is the directory slug; display metadata (name, color) is authored in
 * each pack's `pack.json` and surfaced via `PackMeta`, not hardcoded here —
 * packs are data added by ingestion, not a fixed code-level set.
 */
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
}

export interface UserMeta {
  favorite: boolean;
  tags: string[];
}

export interface AnimationMeta {
  clipCount: number;
  clipNames: string[];
}

interface AssetBase {
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

export type Asset = ModelAsset | MaterialAsset;

export type AssetKind = Asset["kind"];

export const isModel = (a: Asset): a is ModelAsset => a.kind === "model";

export interface Catalog {
  schemaVersion: number;
  libraryRoot: string;
  assets: Asset[];
}
