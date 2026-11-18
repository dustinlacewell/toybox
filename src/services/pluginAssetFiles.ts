/**
 * File access the host hands to a plugin-implemented asset type: the files of
 * one plugin asset, by name. A plugin never passes a library path — it names a
 * file of the asset it was given, and the host resolves it.
 */

import type { AssetFilesApi } from "@ldlework/toybox-sdk";
import { assetUrl } from "./assetUrl";

/** The library of an asset handed to a plugin (the qualified id names it). */
const libraryOf = (asset: { library?: string; id: string }): string => asset.library ?? asset.id.split("::")[0];

/** The library-relative path of `file` within `asset.files`. `file` may be a
 *  bare name or the full relative path; absent, the asset's first file. */
export function resolveAssetFile(asset: { name: string; files: string[] }, file?: string): string {
  if (!asset.files.length) throw new Error(`${asset.name} has no files`);
  if (file === undefined) return asset.files[0];
  const found = asset.files.find((path) => path === file || path.slice(path.lastIndexOf("/") + 1) === file);
  if (!found) throw new Error(`${asset.name} has no file named ${file}`);
  return found;
}

export const pluginAssetFiles: AssetFilesApi = {
  fileUrl: (asset, file) => assetUrl(libraryOf(asset), resolveAssetFile(asset, file)),
  readFile: async (asset, file) => {
    const path = resolveAssetFile(asset, file);
    const response = await fetch(await assetUrl(libraryOf(asset), path));
    if (!response.ok) throw new Error(`Could not read ${path} (${response.status})`);
    return new Uint8Array(await response.arrayBuffer());
  },
};
