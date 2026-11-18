/**
 * Running tag edits: the library that holds each copy of an asset makes its own
 * change (a remote one through its server), the catalog follows from the answers,
 * and what could not be saved is reported per library. The planning is pure, see
 * `domain/tagEditing`.
 */

import type { AssetMetadataEdit } from "../domain/libraryTags";
import { chunk, planPersonalMigration, type LibraryOutcome, type TagPlan } from "../domain/tagEditing";
import { parseQualified } from "../domain/libraryId";
import { unwritableReason } from "../domain/transfer";
import { replaceCachedAssets } from "../state/assetCache";
import { useStore } from "../state/store";
import { dropPersonalTags, personalTagAssets } from "./browse";
import { updateAssetMetadata } from "./tauriApi";

const reason = (failure: unknown) => String(failure instanceof Error ? failure.message : failure).replace(/^Error: /, "");

/** Why this library cannot be written (plain words), or null when it can. */
export function writeReason(library: string): string | null {
  const info = useStore.getState().libraries.find((candidate) => candidate.id === library);
  return info ? unwritableReason(info) : "the library is not available";
}

export const libraryName = (library: string): string => useStore.getState().libraries.find((candidate) => candidate.id === library)?.name ?? library;

/** Save the edits of one library, a call per `MAX_EDITS_PER_CALL`; the catalog follows each answer. */
async function saveLibrary(library: string, edits: AssetMetadataEdit[], advance: (count: number) => void): Promise<LibraryOutcome> {
  let saved = 0;
  for (const part of chunk(edits)) {
    try {
      replaceCachedAssets(await updateAssetMetadata(part));
      saved += part.length;
      advance(part.length);
    } catch (failure) {
      return { library, saved, error: reason(failure) };
    }
  }
  return { library, saved };
}

/**
 * Run a plan, library by library. A library that fails does not stop the others;
 * `onProgress` counts edits (saved or given up on) out of all of them.
 */
export async function runTagPlan(plan: TagPlan, onProgress?: (progress: { done: number; total: number }) => void): Promise<LibraryOutcome[]> {
  const total = [...plan.edits.values()].reduce((sum, edits) => sum + edits.length, 0);
  let done = 0;
  const advance = (count: number) => { done += count; onProgress?.({ done, total }); };
  onProgress?.({ done, total });
  const outcomes: LibraryOutcome[] = [];
  try {
    for (const [library, edits] of plan.edits) {
      const outcome = await saveLibrary(library, edits, advance);
      if (outcome.error) advance(edits.length - outcome.saved);
      outcomes.push(outcome);
    }
  } finally {
    // The tag counts, the search and the lists that filter by tag follow what was saved.
    useStore.getState().bumpData();
  }
  return outcomes;
}

/**
 * Personal tags that older versions kept only on this computer become library
 * tags of remote libraries that can be written (a local library does this in the
 * backend when it is loaded). Anything that cannot be moved stays, shown and
 * searchable. Quiet and best effort: a failure leaves the tags where they were
 * and it is tried again at the next load.
 */
export async function migratePersonalTagsOfRemoteLibraries(): Promise<void> {
  try {
    const { libraries } = useStore.getState();
    const remote = new Set(libraries.filter((library) => library.kind === "remote" && unwritableReason(library) === null).map((library) => library.id));
    if (!remote.size) return;
    // Only the items that carry personal tags are read (a handful: these are tags older versions kept on this computer).
    const found = (await Promise.all([...remote].map((library) => personalTagAssets(library, 5000)))).flat();
    const plan = planPersonalMigration(found, (library) => remote.has(library));
    if (!plan.taken.size) return;
    const outcomes = await runTagPlan({ edits: plan.edits, blocked: new Map(), changed: 0 });
    const failed = new Set(outcomes.filter((outcome) => outcome.error).map((outcome) => outcome.library));
    const moved = new Map<string, { id: string; tags: string[] }[]>();
    for (const [id, tags] of plan.taken) {
      const parsed = parseQualified(id);
      if (!parsed || failed.has(parsed.library)) continue;
      moved.set(parsed.library, [...(moved.get(parsed.library) ?? []), { id: parsed.id, tags }]);
    }
    for (const [library, removals] of moved) await dropPersonalTags(library, removals);
    if (moved.size) useStore.getState().bumpData();
  } catch (failure) {
    console.warn("Could not move personal tags into the libraries", failure);
  }
}
