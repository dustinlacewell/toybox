/**
 * Recursively dispose a three.js scene graph's GPU resources (geometries,
 * materials, textures). Shared by the viewer (on close) and the thumbnail
 * renderer (after each capture) to keep the WebGL context from leaking across
 * many loads.
 */

import { Material, Mesh, Object3D, Texture } from "three";

export function disposeScene(root: Object3D): void {
  root.traverse((obj) => {
    const mesh = obj as Mesh;
    mesh.geometry?.dispose?.();
    const material = mesh.material;
    if (Array.isArray(material)) material.forEach(disposeMaterial);
    else if (material) disposeMaterial(material);
  });
}

function disposeMaterial(material: Material): void {
  for (const value of Object.values(material)) {
    if (value instanceof Texture) value.dispose();
  }
  material.dispose();
}
