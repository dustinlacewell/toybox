/**
 * Where a live preview gets its images. A map names its image by file name; this
 * finds that file in the draft and loads it. Pure: the load itself is injected.
 */

import type { MaterialDraft } from "./draft.js";

export function previewUrlResolver(
  draft: MaterialDraft,
  loadThumb: (relPath: string) => Promise<string>,
): (file: string) => Promise<string> {
  return (file) => {
    const found = draft.files.find((f) => f.fileName === file);
    return found ? loadThumb(found.relPath) : Promise.reject(new Error(`unknown file: ${file}`));
  };
}
