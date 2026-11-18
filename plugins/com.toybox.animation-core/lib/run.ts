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
  skipped: number;
  warnings: string[];
}

export async function importAnimations(
  api: ModelApi,
  sourceRoot: string,
  libraryRoot: string,
  files: ModelFile[],
  pack: string,
  category: string,
): Promise<AnimationImport> {
  const entries: AnimationSeedEntry[] = [];
  const warnings: string[] = [];
  const seenStems = new Set<string>();
  let skipped = 0;
  for (const f of files) {
    // Two sources with the same basename collide on the stored file and the id.
    if (seenStems.has(f.stem)) {
      skipped += 1;
      warnings.push(`skipped duplicate name: ${f.relPath}`);
      continue;
    }
    const result = await api.convert(sourceRoot, libraryRoot, f, pack, category);
    if ("rejected" in result) {
      skipped += 1;
      warnings.push(`skipped ${f.relPath}: ${result.rejected}`);
      continue;
    }
    seenStems.add(f.stem);
    warnings.push(...result.warnings);
    if (!(await api.hasAnimationClips(libraryRoot, pack, category, result.file))) {
      skipped += 1;
      warnings.push(`No animation clips in ${f.relPath}`);
      continue;
    }
    entries.push({ kind: "animation", id: `uid://import_${pack}_${category}_animations_${f.stem}`, pack, category, file: result.file, format: f.ext });
  }
  return { entries, skipped, warnings };
}
