/**
 * "Open substance" on a material's Substance tab opens the substance viewer with
 * that material chosen to edit. The tab and the viewer are separate modules of
 * this plugin, loaded by the host from the same plugin URL, so they share this
 * module instance: the tab leaves the request here and the viewer takes it when
 * it mounts. A request is used once.
 */

import { rawId } from "../lib/substance.js";

const pending = new Map<string, string>();

/** Ask the viewer of `substanceId` to start with `materialId` as the material being edited. */
export function requestEdit(substanceId: string, materialId: string): void {
  pending.set(rawId(substanceId), rawId(materialId));
}

/** The material the viewer of `substanceId` was asked to start with, if any (forgotten once taken). */
export function takeEdit(substanceId: string): string | undefined {
  const key = rawId(substanceId);
  const id = pending.get(key);
  pending.delete(key);
  return id;
}
