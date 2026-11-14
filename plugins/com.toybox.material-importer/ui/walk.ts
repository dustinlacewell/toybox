/**
 * Source-tree discovery and file copying for the material importer. Thin I/O over
 * the host's jailed fs: list every file under a picked folder, and copy one
 * material's used images into `library/<pack>/materials/<name>/`. Free of React.
 */

import type { FsApi } from "@ldlework/toybox-sdk";
import type { MaterialDraft } from "./draft.js";
import { usedFiles } from "./entry.js";

/** The host capabilities `copyMaterial` needs. */
export interface Imports {
  fs: FsApi;
  sourceRoot: string;
  libraryRoot: string;
}

/** Recursively list every file under `sourceRoot` as source-relative, forward-slashed
 *  paths. Extension filtering is left to grouping. */
export async function findImages(fs: FsApi, sourceRoot: string): Promise<string[]> {
  const out: string[] = [];
  const queue: string[] = ["."];
  while (queue.length) {
    const dir = queue.shift()!;
    for (const e of await fs.readDir(sourceRoot, dir)) {
      if (e.isDir) queue.push(e.relPath);
      else out.push(e.relPath);
    }
  }
  return out;
}

/** Copy each used image of `draft` into `library/<pack>/materials/<name>/`. */
export async function copyMaterial(io: Imports, draft: MaterialDraft, pack: string): Promise<void> {
  const destDir = `library/${pack}/materials/${draft.name}`;
  for (const f of usedFiles(draft)) {
    const bytes = await io.fs.readBytes(io.sourceRoot, f.relPath);
    await io.fs.writeBytes(io.libraryRoot, `${destDir}/${f.fileName}`, bytes);
  }
}
