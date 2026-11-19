/**
 * The animation import for paths given on the command line: folders are scanned
 * and files named one by one, then handed to `importAnimations` (the same code
 * the import panel runs). Kept apart from `run.ts`, which has no runtime imports.
 */

import { assertCategory, importNamespace, mergeImportOutcome, newImportOutcome, noteSkip, resolveSources, type FsApi } from "@ldlework/toybox-sdk";
import type { ModelApi, ModelFile } from "com.toybox.model-core";

import { importAnimations, type AnimationImport } from "./run.js";

/** The whole import for paths given as folders and model files: a folder is
 *  scanned by the Models plugin for every format that is turned on; a named
 *  file whose extension no format handles is left out. Files without animation
 *  clips are skipped, as in the panel. */
export async function importAnimationPaths(
  api: ModelApi,
  io: { fs: FsApi; libraryRoot: string },
  paths: string[],
  options: { category: string },
  progress?: (message: string) => void,
): Promise<AnimationImport> {
  const { category } = options;
  assertCategory(category);
  const extensions = new Set(api.formats().flatMap((format) => format.extensions.map((e) => e.replace(/^\.+/, "").toLowerCase())));
  const total: AnimationImport = newImportOutcome();
  for (const source of await resolveSources(io.fs, paths)) {
    let files: ModelFile[] = [];
    if (source.files) {
      for (const relPath of source.files) {
        const name = relPath.slice(relPath.lastIndexOf("/") + 1);
        const ext = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1).toLowerCase() : "";
        if (ext && extensions.has(ext)) files.push({ relPath, stem: name.replace(/\.[^.]+$/, ""), ext });
        else {
          noteSkip(total, `skipped ${relPath}: no turned-on plugin reads this file. ${api.lookingFor()}`);
        }
      }
    } else {
      files = await api.scan(source.root);
    }
    const result = await importAnimations(api, source.root, io.libraryRoot, files, importNamespace(source.root), category, progress);
    mergeImportOutcome(total, result);
  }
  return total;
}
