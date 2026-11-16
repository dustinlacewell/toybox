/**
 * Recursively dispose a three.js scene graph's GPU resources (geometries,
 * materials, textures). Shared by the viewer (on close) and the thumbnail
 * renderer (after each capture) to keep the WebGL context from leaking across
 * many loads.
 */

import { Material, Mesh, Object3D, Texture, type SkinnedMesh, type Skeleton } from "three";

import { materialTextures } from "./materialTextures";

const sharedResources=new WeakSet<object>();
/** Preview-template geometry and materials are reused; each clone owns bones. */
export function shareSceneResources(root:Object3D):void {
  root.traverse(obj=>{const mesh=obj as Mesh;if(mesh.geometry)sharedResources.add(mesh.geometry);
    for(const material of Array.isArray(mesh.material)?mesh.material:mesh.material?[mesh.material]:[]){sharedResources.add(material);for(const texture of materialTextures(material))sharedResources.add(texture);}
  });
}

export function disposeScene(root: Object3D): void {
  // A texture can sit in several materials; dispose each once.
  const seen = new Set<Texture>();
  const skeletons = new Set<Skeleton>();
  root.traverse((obj) => {
    const mesh = obj as Mesh;
    const skeleton=(obj as SkinnedMesh).skeleton;
    if(skeleton&&!skeletons.has(skeleton)){skeletons.add(skeleton);skeleton.dispose();}
    if(mesh.geometry&&!sharedResources.has(mesh.geometry))mesh.geometry.dispose();
    const material = mesh.material;
    if (Array.isArray(material)) material.forEach((m) => disposeOnce(m, seen));
    else if (material) disposeOnce(material, seen);
  });
}

function disposeOnce(material: Material, seen: Set<Texture>): void {
  if(sharedResources.has(material))return;
  for (const texture of materialTextures(material)) {
    if (seen.has(texture)) continue;
    seen.add(texture);
    if(!sharedResources.has(texture))texture.dispose();
  }
  material.dispose();
}
