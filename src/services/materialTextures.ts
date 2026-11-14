/**
 * Helpers over the textures a material owns. A texture can sit in several slots,
 * so every helper treats each texture once. `envMap` is the renderer's shared
 * environment, never owned by the material, so it is skipped.
 */

import { type Material, Texture } from "three";

/** The unique textures held in the material's slots. */
export function materialTextures(material: Material): Texture[] {
  const found = new Set<Texture>();
  for (const [slot, value] of Object.entries(material)) {
    if (slot === "envMap") continue;
    if (value instanceof Texture) found.add(value);
  }
  return [...found];
}

/** Dispose the material and each of its textures once. */
export function disposeMaterial(material: Material): void {
  materialTextures(material).forEach((texture) => texture.dispose());
  material.dispose();
}
