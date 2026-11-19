import type { ImportResult, ImportRun, SeedEntryInput } from "./index.js";

/** Whether `value` is one safe path segment on every platform: not empty, no
 *  slashes or other reserved characters, not `.` or `..`, no trailing dot or space.
 *  Category names and library folder names must pass. */
export function validSegment(value: string): boolean {
  return !!value.trim() && !/[<>:"/\\|?*\x00-\x1f]/.test(value) && value !== "." && value !== ".." && !/[. ]$/.test(value);
}

/** Throws the standard message when `category` is not a `validSegment`. */
export function assertCategory(category: string): void {
  if (!validSegment(category)) throw new Error("The category must be a name without slashes or other reserved characters.");
}

/** What a headless import returns: the entries to commit, how many files or
 *  items were left out and why (`skippedNotes`, which are also among
 *  `warnings` so a panel can list them in order), and other notes. */
export interface ImportOutcome<E extends { id: string } = SeedEntryInput> {
  entries: E[];
  skipped: number;
  skippedNotes: string[];
  warnings: string[];
}

export function newImportOutcome<E extends { id: string } = SeedEntryInput>(): ImportOutcome<E> {
  return { entries: [], skipped: 0, skippedNotes: [], warnings: [] };
}

/** Record one skipped file or item in `total`. */
export function noteSkip(total: ImportOutcome<{ id: string }>, note: string): void {
  total.skipped += 1;
  total.skippedNotes.push(note);
  total.warnings.push(note);
}

/** Add the outcome of one source to the total of an import over several. An
 *  entry whose id is already in the total is dropped (the same file named twice). */
export function mergeImportOutcome<E extends { id: string }>(total: ImportOutcome<E>, part: ImportOutcome<E>): void {
  const seen = new Set(total.entries.map((entry) => entry.id));
  for (const entry of part.entries) if (!seen.has(entry.id)) { seen.add(entry.id); total.entries.push(entry); }
  total.skipped += part.skipped;
  total.skippedNotes.push(...part.skippedNotes);
  total.warnings.push(...part.warnings);
}

/**
 * The shared body of a command-line importer handler: find the library folder,
 * run `work` (the plugin's headless import), fail with `nothingFound(outcome)`
 * when it produced no entries, commit, and report as an `ImportResult`
 * (the skip notes as `skipped`, the other warnings as `warnings`).
 */
export async function runImport<E extends SeedEntryInput>(
  run: ImportRun,
  work: (libraryRoot: string) => Promise<ImportOutcome<E>>,
  nothingFound: (outcome: ImportOutcome<E>) => string,
): Promise<ImportResult> {
  const libraryRoot = await run.host.getLibraryRoot();
  if (!libraryRoot) throw new Error("The library has no folder to import into.");
  const outcome = await work(libraryRoot);
  if (!outcome.entries.length) throw new Error(nothingFound(outcome));
  await run.commit(outcome.entries);
  return {
    imported: outcome.entries.map((entry) => entry.id),
    skipped: outcome.skippedNotes,
    warnings: outcome.warnings.filter((warning) => !outcome.skippedNotes.includes(warning)),
  };
}
