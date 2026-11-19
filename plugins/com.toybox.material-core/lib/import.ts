/**
 * The material import, free of any user interface: stage the images of the
 * material drafts that can be imported and build their seed entries. The import
 * panel calls `importMaterialItems` with the drafts the user reviewed;
 * `tbx import material` calls `importMaterialPaths`, which finds and groups the
 * drafts itself the way the panel's first scan does. The caller commits.
 */

import { assertCategory, groupSeedVariants, importNamespace, mergeImportOutcome, newImportOutcome, noteSkip, resolveSources, type FsApi, type SeedEntryInput } from "@ldlework/toybox-sdk";

import { importable, problemsFor, type ReviewItem } from "./edit.js";
import { toEntry } from "./entry.js";
import { groupMaterials } from "./group.js";
import { copyMaterial, findImages } from "./walk.js";

export interface MaterialImportOutcome {
  /** Entries to commit, with detail levels of one material grouped as variants. */
  entries: SeedEntryInput[];
  /** How many included materials were left out; `skippedNotes` says why. */
  skipped: number;
  skippedNotes: string[];
  /** Notes worth showing (a material that could not be copied is also in `skippedNotes`). */
  warnings: string[];
}

/** Copy the used images of every importable, included item into
 *  `library/<pack>/materials/<name>/` and build its entry. */
export async function importMaterialItems(
  io: { fs: FsApi; sourceRoot: string; libraryRoot: string },
  items: ReviewItem[],
  pack: string,
  category: string,
  progress?: (message: string) => void,
): Promise<MaterialImportOutcome> {
  const outcome: MaterialImportOutcome = { entries: [], skipped: 0, skippedNotes: [], warnings: [] };
  const entries: SeedEntryInput[] = [];
  items.forEach((item, index) => {
    if (!item.included) return;
    const problems = problemsFor(items, index, pack, category);
    if (problems.length) outcome.skippedNotes.push(`skipped ${item.draft.name}: ${problems.join(", ")}`);
  });
  const ready = importable(items, pack, category);
  let done = 0;
  for (const { draft } of ready) {
    progress?.(`Copying ${++done} of ${ready.length}: ${draft.name}`);
    const res = toEntry(draft, pack, category);
    if (!("entry" in res)) continue;
    try {
      await copyMaterial(io, draft, pack);
      entries.push(res.entry);
    } catch (failure) {
      const note = `skipped ${draft.name}: ${String(failure)}`;
      outcome.skippedNotes.push(note);
      outcome.warnings.push(note);
    }
  }
  outcome.entries = groupSeedVariants(entries);
  outcome.skipped = outcome.skippedNotes.length;
  return outcome;
}

/** The whole import for paths given as folders and image files: a folder is
 *  searched, its images grouped into materials by their names and every
 *  material that can be imported is. Files named one by one are grouped with the
 *  others from the same folder. */
export async function importMaterialPaths(
  io: { fs: FsApi; libraryRoot: string },
  paths: string[],
  options: { category: string },
  progress?: (message: string) => void,
): Promise<MaterialImportOutcome> {
  assertCategory(options.category);
  const total: MaterialImportOutcome = newImportOutcome();
  for (const source of await resolveSources(io.fs, paths)) {
    const found = source.files ?? (await findImages(io.fs, source.root));
    if (source.files) for (const file of found.filter((name) => !/\.(png|jpe?g)$/i.test(name))) noteSkip(total, `skipped ${file}: not a PNG or JPG image`);
    const items: ReviewItem[] = groupMaterials(found).map((draft) => ({ draft, included: true }));
    const outcome = await importMaterialItems({ fs: io.fs, sourceRoot: source.root, libraryRoot: io.libraryRoot }, items, importNamespace(source.root), options.category, progress);
    mergeImportOutcome(total, outcome);
  }
  return total;
}
