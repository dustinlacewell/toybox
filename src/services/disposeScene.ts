/**
 * Recursively dispose a three.js scene graph's GPU resources (geometries,
 * materials, textures). Shared by the viewer (on close) and the thumbnail
 * renderer (after each capture) to keep the WebGL context from leaking across
 * many loads.
 */

import { Material, Mesh, Object3D, Texture } from "three";

import { materialTextures } from "./materialTextures";

export function disposeScene(root: Object3D): void {
  // A texture can sit in several materials; dispose each once.
  const seen = new Set<Texture>();
  root.traverse((obj) => {
    const mesh = obj as Mesh;
    mesh.geometry?.dispose?.();
    const material = mesh.material;
    if (Array.isArray(material)) material.forEach((m) => disposeOnce(m, seen));
    else if (material) disposeOnce(material, seen);
  });
}

function disposeOnce(material: Material, seen: Set<Texture>): void {
  for (const texture of materialTextures(material)) {
    if (seen.has(texture)) continue;
    seen.add(texture);
    texture.dispose();
  }
  material.dispose();
}
