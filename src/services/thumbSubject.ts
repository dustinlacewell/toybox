/**
 * The per-kind seam of thumbnail rendering: what object a thumbnail shows for
 * an asset. One loader per `AssetKind`; the compiler rejects a missing kind.
 */

import { AnimationMixer, Group, type Object3D, type WebGLRenderer } from "three";
import { createRigGuide, hasVisibleGeometry } from "./rigGuide";

import type { Asset, BuiltinKind } from "../domain/catalog";
import { loadAssetScene } from "./gltfLoad";
import { loadMaterial } from "./materialLoad";
import { buildPreviewMesh } from "./materialPreview";
import { studioEnvironment } from "./studioEnvironment";
import { usePreviewPreferences } from "../state/previewPreferences";

type KindOf<K extends BuiltinKind> = Extract<Asset, { kind: K }>;
/** The kinds whose thumbnail is a rendered 3D subject. Audio draws a waveform
 *  and a plugin type renders its own picture. */
type SceneKind = Exclude<BuiltinKind,"audio">;

type SubjectLoaders = {
  [K in SceneKind]: (asset: KindOf<K>, renderer: WebGLRenderer) => Promise<Object3D>;
};

const MATERIAL_THUMB_TEXTURE_SIZE = 512;

const LOADERS: SubjectLoaders = {
  animation: async (asset) => {
    const { scene, clips } = await loadAssetScene(asset,{sampleTime:asset.clip.duration*0.25});
    if (clips[0]) { const mixer = new AnimationMixer(scene); mixer.clipAction(clips[0]).play(); mixer.setTime(clips[0].duration * 0.25); }
    scene.updateWorldMatrix(true, true);
    if (hasVisibleGeometry(scene)) return scene;
    const result = new Group(); result.add(scene, createRigGuide(scene).object); return result;
  },
  // Thumbnails are bind-pose stills; animation clips are ignored.
  model: async (asset) => (await loadAssetScene(asset)).scene,
  material: async (asset, renderer) => {
    const shape=usePreviewPreferences.getState().materialStyle;
    const material = await loadMaterial(asset, { maxSize: MATERIAL_THUMB_TEXTURE_SIZE });
    material.envMap = studioEnvironment(renderer);
    return buildPreviewMesh(material, shape);
  },
};

/** Build the object a thumbnail shows for an asset. Caller owns disposal. */
export function loadThumbSubject(asset: KindOf<SceneKind>, renderer: WebGLRenderer): Promise<Object3D> {
  const load = LOADERS[asset.kind] as (a: Asset, r: WebGLRenderer) => Promise<Object3D>;
  return load(asset, renderer);
}
