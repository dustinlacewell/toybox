/**
 * Demo shim for services/assetUrl. The real module resolves library-relative
 * paths to Tauri asset:// URLs; here every fileset path maps to a bundled file
 * under /demo-assets, and thumbnails come from the in-memory store the demo's
 * tauriApi shim fills as the (real) renderer produces them.
 *
 * Library-relative paths look like "polygon_prototype/props/Foo.gltf" — we
 * strip the leading pack segment, since the demo bundles a single pack flat
 * under /demo-assets/<category>/...
 */

import { getThumbUrl } from "./thumbStore";

const BASE = `${import.meta.env.BASE_URL.replace(/\/$/, "")}/demo-assets`;

/** URL for a library-relative file (gltf, bin, texture). */
export async function assetUrl(relPath: string): Promise<string> {
  const norm = relPath.replace(/\\/g, "/");
  // Drop the pack slug: "<pack>/<category>/<file>" -> "<category>/<file>".
  const withoutPack = norm.replace(/^[^/]+\//, "");
  return `${BASE}/${withoutPack}`;
}

/** Object URL for an asset's freshly-rendered thumbnail, or a 1x1 fallback. */
export async function thumbUrl(assetId: string): Promise<string> {
  return getThumbUrl(assetId) ?? "data:image/gif;base64,R0lGODlhAQABAAAAACw=";
}
