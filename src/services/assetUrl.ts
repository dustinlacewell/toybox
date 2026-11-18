/**
 * Turn library-relative and app-data paths into webview-loadable URLs via the
 * Tauri asset protocol. The absolute paths come from the Rust path-resolution
 * commands; every registered library root and every thumb dir is in the
 * asset-protocol scope (see tauri.conf.json and `infra::library`).
 *
 * A library-relative path means nothing without its library: callers pass
 * `asset.library` (several libraries can hold the same relative path).
 */

import { convertFileSrc } from "@tauri-apps/api/core";

import { resolveAssetPath, thumbPath } from "./tauriApi";

/** asset:// URL for a library-relative file (gltf, bin, texture) of `library`. */
export async function assetUrl(library: string, relPath: string): Promise<string> {
  const abs = await resolveAssetPath(library, relPath);
  return convertFileSrc(abs);
}

/** asset:// URL for an asset's cached thumbnail PNG (qualified asset id). */
export async function thumbUrl(assetId: string): Promise<string> {
  const abs = await thumbPath(assetId);
  return convertFileSrc(abs);
}
