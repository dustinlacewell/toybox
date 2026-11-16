import { Light, Mesh, MeshStandardMaterial, type Scene } from "three";
import { PREVIEW_BRIGHTNESS, type PreviewBrightness } from "../domain/previewBrightness";

const original = new WeakMap<object, number>();
function scaled(owner: object, value: number, factor: number) {
  if (!original.has(owner)) original.set(owner, value);
  return original.get(owner)! * factor;
}

/** Dim illumination without changing emissive materials or reparsing a scene. */
export function setPreviewLighting(scene: Scene, brightness: PreviewBrightness) {
  const factor=PREVIEW_BRIGHTNESS[brightness];
  scene.environmentIntensity=scaled(scene,scene.environmentIntensity,factor);
  scene.traverse(node=>{
    if(node instanceof Light)node.intensity=scaled(node,node.intensity,factor);
    if(node instanceof Mesh)for(const material of Array.isArray(node.material)?node.material:[node.material]) {
      if(material instanceof MeshStandardMaterial&&material.envMap)material.envMapIntensity=scaled(material,material.envMapIntensity,factor);
    }
  });
}
