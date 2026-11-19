/**
 * The model import, free of any user interface: hand each chosen file to the
 * format plugin that claimed its extension, keep the originals of converted
 * models, and build the seed entries. The import panel calls `importModelFiles`
 * with the files the user ticked; `tbx import model` calls `importModelPaths`,
 * which finds the files itself. The caller commits.
 */

import { assertCategory, groupSeedVariants, importNamespace, mergeImportOutcome, newImportOutcome, noteSkip, resolveSources, type FsApi, type SeedEntryInput } from "@ldlework/toybox-sdk";

import { chooseModels, groupModels, type LevelPolicy } from "./choose.js";
import { hasFormats, lookingFor, modelExtension, NO_FORMATS, type FormatTable } from "./formats.js";
import { newStagedNames } from "./originals.js";
import { findModels, keepOriginal, materialize, type Found } from "./walk.js";

export interface ModelImportOutcome {
  /** Entries to commit, with detail levels of one model grouped as variants. */
  entries: SeedEntryInput[];
  /** How many files were left out; `skippedNotes` says why. */
  skipped: number;
  skippedNotes: string[];
  /** Everything worth telling the user, including the `skippedNotes`. */
  warnings: string[];
}

/** Convert `found` (under `io.sourceRoot`) into `library/<pack>/<category>/` and
 *  return the entries to commit. */
export async function importModelFiles(
  io: { fs: FsApi; formats: FormatTable; sourceRoot: string; libraryRoot: string },
  found: Found[],
  pack: string,
  category: string,
  progress?: (message: string) => void,
): Promise<ModelImportOutcome> {
  const entries: SeedEntryInput[] = [];
  const outcome: ModelImportOutcome = { entries, skipped: 0, skippedNotes: [], warnings: [] };
  const skip = (note: string) => { outcome.skipped += 1; outcome.skippedNotes.push(note); outcome.warnings.push(note); };
  const seenStems = new Set<string>();
  const stagedNames = newStagedNames();
  let done = 0;

  for (const f of found) {
    progress?.(`Importing ${++done} of ${found.length}: ${f.relPath}`);
    // Two sources with the same basename collide on both the on-disk
    // `<stem>.gltf` and the catalog id, so the second would silently
    // overwrite/duplicate. Keep the first, skip the rest.
    if (seenStems.has(f.stem)) { skip(`skipped duplicate name: ${f.relPath}`); continue; }

    const result = await materialize(io, f, pack, category);
    if ("rejected" in result) { skip(`skipped ${f.relPath}: ${result.rejected}`); continue; }
    seenStems.add(f.stem);
    outcome.warnings.push(...result.warnings);
    // A converted model keeps its original beside the glTF; a format whose
    // output is the original (glTF) only records which plugin imported it.
    const kept = await keepOriginal(io, f, result, pack, category, stagedNames);
    outcome.warnings.push(...kept.warnings);
    entries.push({
      id: `uid://import_${pack}_${category}_${f.stem}`,
      pack,
      category,
      file: result.file,
      format: f.ext,
      ...(kept.origin ? { origin: kept.origin } : {}),
    });
  }
  outcome.entries = groupSeedVariants(entries);
  return outcome;
}

/** The whole import for paths given as folders and model files. Folders are
 *  searched for every extension a turned-on format handles; a named file whose
 *  extension nothing handles is left out. */
export async function importModelPaths(
  io: { fs: FsApi; formats: FormatTable; libraryRoot: string },
  paths: string[],
  options: { category: string; levels: LevelPolicy },
  progress?: (message: string) => void,
): Promise<ModelImportOutcome> {
  if (!hasFormats(io.formats)) throw new Error(NO_FORMATS);
  const { category } = options;
  assertCategory(category);
  const total: ModelImportOutcome = newImportOutcome();
  for (const source of await resolveSources(io.fs, paths)) {
    let found: Found[];
    if (source.files) {
      found = [];
      for (const relPath of source.files) {
        const name = relPath.slice(relPath.lastIndexOf("/") + 1);
        const ext = modelExtension(name, io.formats);
        if (ext) found.push({ relPath, stem: name.replace(/\.[^.]+$/, ""), ext });
        else {
          noteSkip(total, `skipped ${relPath}: no turned-on plugin reads this file. ${lookingFor(io.formats)}`);
        }
      }
    } else {
      found = await findModels(io.fs, source.root, io.formats);
    }
    const chosen = chooseModels(groupModels(found), options.levels);
    const outcome = await importModelFiles({ fs: io.fs, formats: io.formats, sourceRoot: source.root, libraryRoot: io.libraryRoot }, chosen, importNamespace(source.root), category, progress);
    mergeImportOutcome(total, outcome);
  }
  return total;
}
