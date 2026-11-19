import type { ImportResult, ImportRun, ReplacedItem, SeedEntryInput } from "./index.js";

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
  /** Items the import left alone because the library already has them (not an error). */
  unchanged?: string[];
  /** Items among `entries` that replace an item the library already had. */
  replaced?: ReplacedItem[];
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
  if (part.unchanged?.length) (total.unchanged ??= []).push(...part.unchanged);
  if (part.replaced?.length) (total.replaced ??= []).push(...part.replaced);
}

/**
 * The shared body of a command-line importer handler: find the library folder,
 * run `work` (the plugin's headless import), fail with `nothingFound(outcome)`
 * when it produced no entries, commit, and report as an `ImportResult`
 * (the skip notes as `skipped`, the other warnings as `warnings`). Items the library
 * already had (`unchanged`) are not errors: they are mentioned and left alone.
 */
export async function runImport<E extends SeedEntryInput>(
  run: ImportRun,
  work: (libraryRoot: string) => Promise<ImportOutcome<E>>,
  nothingFound: (outcome: ImportOutcome<E>) => string,
): Promise<ImportResult> {
  const libraryRoot = await run.host.getLibraryRoot();
  if (!libraryRoot) throw new Error("The library has no folder to import into.");
  const outcome = await work(libraryRoot);
  const unchanged = outcome.unchanged ?? [];
  if (!outcome.entries.length && !unchanged.length) throw new Error(nothingFound(outcome));
  if (unchanged.length) run.log(`Left ${unchanged.length === 1 ? "1 item" : `${unchanged.length} items`} alone, already in the library: ${unchanged.join(", ")}`);
  if (outcome.entries.length) await run.commit(outcome.entries);
  return {
    imported: outcome.entries.map((entry) => entry.id),
    ...(outcome.replaced?.length ? { replaced: outcome.replaced } : {}),
    skipped: outcome.skippedNotes,
    warnings: outcome.warnings.filter((warning) => !outcome.skippedNotes.includes(warning)),
  };
}
