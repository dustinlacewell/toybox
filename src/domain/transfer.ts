/**
 * Transferring items between libraries: the pure parts. No runtime imports (the
 * tests load this file on its own); the IPC lives in `services/tauriApi.ts`.
 *
 * The backend copies or moves ONE source library's items per call. A selection
 * may span several libraries, so it is grouped per source library, items that
 * already sit in the target library are set aside, and the per-library reports
 * are merged into one.
 */

import type { Asset, LibraryInfo } from "./catalog";

export type TransferMode = "copy" | "move";
export type OnConflict = "skip" | "replace";
export type TransferOutcome = "copied" | "replaced" | "skipped" | "failed";

/** One line of the backend's report (raw id, source library's own). */
export interface TransferItem {
  id: string;
  name: string;
  kind: string;
  outcome: TransferOutcome;
  /** Why it was skipped or failed, or a note on one that was copied. */
  reason?: string;
  /** A move deleted it from the source library. */
  sourceRemoved: boolean;
}

export interface TransferReport { items: TransferItem[]; }

/** A report line with the library it came from. `assetId` is the qualified id. */
export interface MergedTransferItem extends TransferItem {
  library: string;
  assetId: string;
}

export interface MergedTransferReport {
  items: MergedTransferItem[];
  copied: number;
  replaced: number;
  skipped: number;
  failed: number;
  /** Items deleted from their source library by a move. */
  removed: number;
}

/** Why a library cannot be written to, in plain words; null when it can. */
export function unwritableReason(library: LibraryInfo): string | null {
  if (library.remote) {
    if (!library.available) return "the server has not been reachable yet and nothing is saved from it";
    if (library.remote.offline || library.offline) return "the server cannot be reached right now";
    if (library.remote.role === "read" || !library.canWrite) return "your access token can only read";
    return null;
  }
  if (!library.available) return "its folder cannot be read right now";
  if (!library.canWrite) return "it is read-only";
  return null;
}

/** Why the files of a library cannot be edited in place (origin alignment and the
 *  like), in plain words; null when they can. A remote library's files are never
 *  edited in place yet: the tools write straight into the folder. */
export function fileEditReason(library: LibraryInfo): string | null {
  if (library.remote) return unwritableReason(library) ?? "files of a remote library cannot be edited in place yet; copy the item to a local library to do that";
  const write = unwritableReason(library);
  if (write) return write;
  return library.canEditFiles === false ? "its files cannot be edited" : null;
}

/** The libraries items may be put into. */
export const writableLibraries = (libraries: readonly LibraryInfo[]): LibraryInfo[] =>
  libraries.filter((library) => unwritableReason(library) === null);

/** Whether the transfer actions are offered at all: only with another library to go to. */
export const canOfferTransfer = (libraries: readonly LibraryInfo[]): boolean => libraries.length > 1;

/**
 * The libraries a selection can be transferred into. A selection from a single
 * library cannot go back into it; a selection spanning several can go into any
 * of them (what already sits there is skipped). `blocked` lists the rest with
 * the reason, for the dialog to say.
 */
export function transferTargets(
  libraries: readonly LibraryInfo[],
  sourceLibraries: readonly string[],
): { targets: LibraryInfo[]; blocked: { library: LibraryInfo; reason: string }[] } {
  const only = sourceLibraries.length === 1 ? sourceLibraries[0] : null;
  const targets: LibraryInfo[] = [];
  const blocked: { library: LibraryInfo; reason: string }[] = [];
  for (const library of libraries) {
    if (library.id === only) continue;
    const reason = unwritableReason(library);
    if (reason) blocked.push({ library, reason });
    else targets.push(library);
  }
  return { targets, blocked };
}

/** Moving deletes from the source libraries, so every source must be writable.
 *  Returns the libraries that are not, with their reasons. */
export function moveBlockers(libraries: readonly LibraryInfo[], sourceLibraries: readonly string[], target: string): { library: LibraryInfo; reason: string }[] {
  const out: { library: LibraryInfo; reason: string }[] = [];
  for (const id of sourceLibraries) {
    if (id === target) continue;
    const library = libraries.find((item) => item.id === id);
    const reason = library && unwritableReason(library);
    if (library && reason) out.push({ library, reason });
  }
  return out;
}

/**
 * Split a selection for one target. `groups` is the selection grouped by source
 * library (raw ids); the target library's own items need no transfer.
 */
export function planTransfer(
  assets: readonly Asset[],
  target: string,
): { groups: Map<string, Asset[]>; alreadyThere: Asset[] } {
  const groups = new Map<string, Asset[]>();
  const alreadyThere: Asset[] = [];
  for (const asset of assets) {
    if (asset.library === target) { alreadyThere.push(asset); continue; }
    groups.set(asset.library, [...(groups.get(asset.library) ?? []), asset]);
  }
  return { groups, alreadyThere };
}

/** Counts for a one-line summary: "12 items from resources → Second". */
export function summaryLine(assets: readonly Asset[], libraries: readonly LibraryInfo[], target: string): string {
  const names = new Map(libraries.map((library) => [library.id, library.name]));
  const sources = [...new Set(assets.map((asset) => asset.library))];
  const from = sources.map((id) => names.get(id) ?? id).join(", ");
  const count = `${assets.length} ${assets.length === 1 ? "item" : "items"}`;
  return `${count} from ${from} → ${names.get(target) ?? target}`;
}

/** The per-source reports, plus the items that were already in the target, as one report. */
export function mergeTransferReports(
  parts: readonly { library: string; report: TransferReport }[],
  alreadyThere: readonly Asset[],
  targetName: string,
): MergedTransferReport {
  const items: MergedTransferItem[] = [];
  for (const { library, report } of parts) {
    for (const item of report.items) items.push({ ...item, library, assetId: `${library}::${item.id}` });
  }
  for (const asset of alreadyThere) {
    items.push({
      id: asset.id.slice(asset.library.length + 2), name: asset.name, kind: asset.kind, outcome: "skipped",
      reason: `Already in ${targetName}`, sourceRemoved: false, library: asset.library, assetId: asset.id,
    });
  }
  const count = (outcome: TransferOutcome) => items.filter((item) => item.outcome === outcome).length;
  return {
    items, copied: count("copied"), replaced: count("replaced"), skipped: count("skipped"), failed: count("failed"),
    removed: items.filter((item) => item.sourceRemoved).length,
  };
}

/** Every item of a source group reported failed (the call itself rejected). */
export function failedGroup(assets: readonly Asset[], reason: string): TransferReport {
  return {
    items: assets.map((asset) => ({
      id: asset.id.slice(asset.library.length + 2), name: asset.name, kind: asset.kind, outcome: "failed" as const, reason, sourceRemoved: false,
    })),
  };
}

/** Progress across source groups: finished groups count in full, the running one by its own progress. */
export function overallProgress(totals: readonly number[], finished: number, running: { done: number; total: number } | null): { done: number; total: number } {
  const total = totals.reduce((sum, value) => sum + value, 0);
  const done = totals.slice(0, finished).reduce((sum, value) => sum + value, 0) + (running ? Math.min(running.done, running.total) : 0);
  return { done: Math.min(done, total), total };
}

/** Libraries whose catalogs must be re-read after a transfer attempt: the target
 *  always (an attempt that threw part-way may still have landed items), and for
 *  a move the source libraries too. The libraries not involved are left alone. */
export function affectedLibraries(sources: readonly string[], target: string, mode: TransferMode): string[] {
  return [...new Set([target, ...(mode === "move" ? sources : [])])];
}

/** The libraries among those `assets` belong to that cannot be written, with
 *  the reason. Delete and recenter are offered only when this is empty. */
export function readOnlyOwners(libraries: readonly LibraryInfo[], assets: readonly Pick<Asset, "library">[]): { library: LibraryInfo; reason: string }[] {
  const owners = new Set(assets.map((asset) => asset.library));
  const out: { library: LibraryInfo; reason: string }[] = [];
  for (const library of libraries) {
    const reason = owners.has(library.id) ? unwritableReason(library) : null;
    if (reason) out.push({ library, reason });
  }
  return out;
}

/** The libraries among those `assets` belong to whose files cannot be edited in
 *  place, with the reason. Origin alignment is offered only when this is empty. */
export function fileEditBlockers(libraries: readonly LibraryInfo[], assets: readonly Pick<Asset, "library">[]): { library: LibraryInfo; reason: string }[] {
  const owners = new Set(assets.map((asset) => asset.library));
  const out: { library: LibraryInfo; reason: string }[] = [];
  for (const library of libraries) {
    const reason = owners.has(library.id) ? fileEditReason(library) : null;
    if (reason) out.push({ library, reason });
  }
  return out;
}

/** One sentence naming why a change is not offered, or undefined when it is. */
export function readOnlyNotice(action: string, blockers: readonly { library: LibraryInfo; reason: string }[]): string | undefined {
  if (!blockers.length) return undefined;
  return `Cannot ${action}: ${blockers.map(({ library, reason }) => `${library.name} (${reason})`).join(", ")}.`;
}
