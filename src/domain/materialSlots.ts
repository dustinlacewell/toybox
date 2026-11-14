/**
 * Which image channel three's `MeshStandardMaterial` samples for each scalar
 * map slot, and whether a material map must be repacked to fit it. Pure data
 * and functions — no React, three, or tauri imports.
 */

import type { MapChannel, MapRole, MaterialMap } from "./material";

export type ScalarRole = "ao" | "roughness" | "metallic" | "opacity";

const SCALAR_ROLES: readonly MapRole[] = ["ao", "roughness", "metallic", "opacity"];

export const isScalarRole = (role: MapRole): role is ScalarRole =>
  SCALAR_ROLES.includes(role);

/** The channel three reads for each scalar slot (aoMap, roughnessMap, metalnessMap, alphaMap). */
export const SAMPLED_CHANNEL: Record<ScalarRole, MapChannel> = {
  ao: "r",
  roughness: "g",
  metallic: "b",
  opacity: "g",
};

/**
 * True when a scalar map's value is not already in the channel three samples:
 * it is stored inverted, or in a different channel. A scalar map with no
 * channel (a grayscale image) and no invert reads correctly as is.
 */
export function needsRepack(map: MaterialMap): boolean {
  if (!isScalarRole(map.role)) return false;
  if (map.invert) return true;
  return map.channel !== undefined && map.channel !== SAMPLED_CHANNEL[map.role];
}
