/**
 * Turn library-relative and app-data paths into webview-loadable URLs via the
 * Tauri asset protocol. The absolute paths come from the Rust path-resolution
 * commands; both the library root and the thumb dir are in the asset-protocol
 * scope (see tauri.conf.json).
 */

import { convertFileSrc } from "@tauri-apps/api/core";

import { resolveAssetPath, thumbPath } from "./tauriApi";

/** asset:// URL for a library-relative file (gltf, bin, texture). */
export async function assetUrl(relPath: string): Promise<string> {
  const abs = await resolveAssetPath(relPath);
  return convertFileSrc(abs);
}

/** asset:// URL for an asset's cached thumbnail PNG. */
export async function thumbUrl(assetId: string): Promise<string> {
  const abs = await thumbPath(assetId);
  return convertFileSrc(abs);
}
