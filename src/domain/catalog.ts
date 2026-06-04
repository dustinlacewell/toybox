/**
 * Catalog types, mirroring the Rust `catalog_model`. Pure data — no React,
 * three, or tauri imports. The primary key is the Godot `uid://` (basenames
 * collide across packs).
 */

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

export type Category =
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

export interface Asset {
  id: string;
  name: string;
  fileName: string;
  relPath: string;
  pack: Pack;
  category: Category;
  fileset: AssetFileset;
  thumb: ThumbMeta;
  user: UserMeta;
  animation: AnimationMeta;
}

export interface Catalog {
  schemaVersion: number;
  libraryRoot: string;
  assets: Asset[];
}
