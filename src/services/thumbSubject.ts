/**
 * The per-kind seam of thumbnail rendering: what object a thumbnail shows for
 * an asset. One loader per `AssetKind`; the compiler rejects a missing kind.
 */

import type { Object3D, WebGLRenderer } from "three";

import type { Asset, AssetKind } from "../domain/catalog";
import { loadAssetScene } from "./gltfLoad";
import { loadMaterial } from "./materialLoad";
import { buildPreviewMesh } from "./materialPreview";
import { studioEnvironment } from "./studioEnvironment";

type KindOf<K extends AssetKind> = Extract<Asset, { kind: K }>;

type SubjectLoaders = {
  [K in AssetKind]: (asset: KindOf<K>, renderer: WebGLRenderer) => Promise<Object3D>;
};

const MATERIAL_THUMB_TEXTURE_SIZE = 512;

const LOADERS: SubjectLoaders = {
  // Thumbnails are bind-pose stills; animation clips are ignored.
  model: async (asset) => (await loadAssetScene(asset)).scene,
  material: async (asset, renderer) => {
    const material = await loadMaterial(asset, { maxSize: MATERIAL_THUMB_TEXTURE_SIZE });
    material.envMap = studioEnvironment(renderer);
    return buildPreviewMesh(material, "sphere");
  },
};

/** Build the object a thumbnail shows for an asset. Caller owns disposal. */
export function loadThumbSubject(asset: Asset, renderer: WebGLRenderer): Promise<Object3D> {
  const load = LOADERS[asset.kind] as (a: Asset, r: WebGLRenderer) => Promise<Object3D>;
  return load(asset, renderer);
}
