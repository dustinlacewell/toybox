/**
 * Material types, mirroring the Rust `material` domain. Pure data — no React,
 * three, or tauri imports.
 */

export type MapRole =
  | "albedo"
  | "normal"
  | "roughness"
  | "metallic"
  | "ao"
  | "height"
  | "emissive"
  | "opacity";

export type MapChannel = "r" | "g" | "b" | "a";

/** One role of a material, bound to a texture file and optionally one channel of it. */
export interface MaterialMap {
  role: MapRole;
  /** Texture path relative to libraryRoot. Several roles may share one file (packed textures). */
  file: string;
  /** The channel a scalar role reads. Absent = the whole image. */
  channel?: MapChannel;
  /** Scalar role stored inverted (e.g. smoothness in place of roughness). */
  invert: boolean;
}

export interface MaterialSet {
  maps: MaterialMap[];
  /** Normal map uses the DirectX convention (green flipped). */
  flipNormalGreen: boolean;
}
