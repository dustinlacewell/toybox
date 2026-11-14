/**
 * Recursively dispose a three.js scene graph's GPU resources (geometries,
 * materials, textures). Shared by the viewer (on close) and the thumbnail
 * renderer (after each capture) to keep the WebGL context from leaking across
 * many loads.
 */

import { Material, Mesh, Object3D, Texture } from "three";

export function disposeScene(root: Object3D): void {
  // A texture can sit in several slots or materials; dispose each once.
  const seen = new Set<Texture>();
  root.traverse((obj) => {
    const mesh = obj as Mesh;
    mesh.geometry?.dispose?.();
    const material = mesh.material;
    if (Array.isArray(material)) material.forEach((m) => disposeMaterial(m, seen));
    else if (material) disposeMaterial(material, seen);
  });
}

function disposeMaterial(material: Material, seen: Set<Texture>): void {
  for (const [slot, value] of Object.entries(material)) {
    // `envMap` is the renderer's shared environment, never owned by a material.
    if (slot === "envMap") continue;
    if (value instanceof Texture && !seen.has(value)) {
      seen.add(value);
      value.dispose();
    }
  }
  material.dispose();
}
