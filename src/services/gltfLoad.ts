/**
 * Load a catalog asset's glTF into a three.js scene through the Tauri asset
 * protocol. Rather than rely on the browser resolving the glTF's relative URIs
 * (`Name.bin`, `../textures/Foo.png`) against a percent-encoded asset:// URL, we
 * pre-resolve every file in the asset's fileset to an asset:// URL and install a
 * URL modifier that maps each requested URI (by basename) to it. This is exact
 * and avoids any `../` resolution ambiguity in the encoded URL.
 *
 * The lone TGA texture in the library is handled by registering TGALoader on the
 * loading manager.
 */

import { type AnimationClip, Group, LoadingManager } from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { TGALoader } from "three/examples/jsm/loaders/TGALoader.js";

import type { Asset } from "../domain/catalog";
import { assetUrl } from "./assetUrl";

export interface LoadedAsset {
  scene: Group;
  /** Animation clips parsed from the glTF (empty for static assets). */
  clips: AnimationClip[];
}

/**
 * Load the asset's scene graph and animation clips. Caller owns disposal of the
 * returned scene. `cacheBust` (a changing nonce) forces the webview to refetch
 * the .gltf after it's been edited in place (e.g. origin correction); the
 * .bin/textures are untouched by those edits so they need no busting.
 */
export async function loadAssetScene(asset: Asset, cacheBust = 0): Promise<LoadedAsset> {
  const urlByBasename = await resolveFileset(asset);

  const manager = new LoadingManager();
  manager.addHandler(/\.tga$/i, new TGALoader(manager));
  manager.setURLModifier((url) => urlByBasename.get(basename(url)) ?? url);

  const loader = new GLTFLoader(manager);
  const gltfBase = urlByBasename.get(basename(asset.fileset.gltf))!;
  const gltfHref = cacheBust ? `${gltfBase}?v=${cacheBust}` : gltfBase;

  const gltf = await loader.loadAsync(gltfHref);
  return { scene: gltf.scene, clips: gltf.animations ?? [] };
}

/** Map every fileset file's basename -> its asset:// URL. */
async function resolveFileset(asset: Asset): Promise<Map<string, string>> {
  const rels = [asset.fileset.gltf, asset.fileset.bin, ...asset.fileset.textures];
  const entries = await Promise.all(
    rels.map(async (rel) => [basename(rel), await assetUrl(rel)] as const),
  );
  return new Map(entries);
}

function basename(p: string): string {
  const norm = p.replace(/\\/g, "/");
  const i = norm.lastIndexOf("/");
  return i >= 0 ? norm.slice(i + 1) : norm;
}
