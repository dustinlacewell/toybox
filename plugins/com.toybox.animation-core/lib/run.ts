/**
 * The animation import: convert each chosen source file through the Models
 * plugin, keep only those that really hold animation clips, and describe them as
 * seed entries. The source of an animation is replaced by the clips extracted
 * from it, so no original is kept and no `origin` is attached; the host rejects
 * kept files for animation sources. Pure orchestration over the service.
 */

import type { AnimationSeedEntry } from "@ldlework/toybox-sdk";
import type { ModelApi, ModelFile } from "com.toybox.model-core";

export interface AnimationImport {
  entries: AnimationSeedEntry[];
  /** How many files were left out; `skippedNotes` says why (they are also in `warnings`). */
  skipped: number;
  skippedNotes: string[];
  warnings: string[];
}

export async function importAnimations(
  api: ModelApi,
  sourceRoot: string,
  libraryRoot: string,
  files: ModelFile[],
  pack: string,
  category: string,
  progress?: (message: string) => void,
): Promise<AnimationImport> {
  const entries: AnimationSeedEntry[] = [];
  const warnings: string[] = [];
  const skippedNotes: string[] = [];
  const seenStems = new Set<string>();
  let skipped = 0;
  const skip = (note: string) => { skipped += 1; skippedNotes.push(note); warnings.push(note); };
  let done = 0;
  for (const f of files) {
    progress?.(`Reading ${++done} of ${files.length}: ${f.relPath}`);
    // Two sources with the same basename collide on the stored file and the id.
    if (seenStems.has(f.stem)) {
      skip(`skipped duplicate name: ${f.relPath}`);
      continue;
    }
    const result = await api.convert(sourceRoot, libraryRoot, f, pack, category);
    if ("rejected" in result) {
      skip(`skipped ${f.relPath}: ${result.rejected}`);
      continue;
    }
    seenStems.add(f.stem);
    warnings.push(...result.warnings);
    if (!(await api.hasAnimationClips(libraryRoot, pack, category, result.file))) {
      skip(`No animation clips in ${f.relPath}`);
      continue;
    }
    entries.push({ kind: "animation", id: `uid://import_${pack}_${category}_animations_${f.stem}`, pack, category, file: result.file, format: f.ext });
  }
  return { entries, skipped, skippedNotes, warnings };
}
