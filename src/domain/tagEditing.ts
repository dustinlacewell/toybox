/**
 * Editing tags: the pure parts.
 *
 * There is one kind of tag. It belongs to the asset and is stored in the library
 * where the asset's file lives (`Asset.tags`, the library manifest). An asset that
 * exists in several libraries (see `./duplicates`: the copy shown and its
 * `alsoIn` copies) has the tags of ALL its copies: they are shown as a union, and
 * an edit writes the same full list to every copy that can be written, so copies
 * that had drifted apart converge.
 *
 * The backend changes one library per call (a remote one through its server), so
 * a plan groups the edits per library; the result of running a plan is reported
 * per library in plain words. Free of I/O. Imports only `./libraryTags` (tests
 * load both files together).
 */

import type { Asset } from "./catalog";
import { normalizeTag, normalizeTags, personalTags, type AssetMetadataEdit } from "./libraryTags";

/** Edits the backend takes in one call. */
export const MAX_EDITS_PER_CALL = 5000;

/** Why the library with this id cannot be written, in plain words; null when it can. */
export type WriteReason = (library: string) => string | null;

/** What to do with the tags of an asset: add some, remove some (both may be given). */
export interface TagChange {
  add?: readonly string[];
  remove?: readonly string[];
}

/** The tags of an asset as its copies hold them: the shown copy's first, then any the others add. */
export const unionTags = (copies: readonly Pick<Asset, "tags">[]): string[] =>
  normalizeTags(copies.flatMap((copy) => copy.tags ?? []));

/** Tags only this computer holds (left by older versions where they could not be moved into the library). They are shown and searched, not edited. */
export function leftoverTags(copies: readonly Pick<Asset, "user">[], shared: readonly string[]): string[] {
  const known = new Set(shared);
  return [...new Set(copies.flatMap((copy) => personalTags(copy)))].filter((tag) => !known.has(tag));
}

/** The shown asset and its other copies (those the catalog holds), shown copy first. */
export function copiesOf(asset: Asset, alsoIn: readonly { id: string }[], lookup: (id: string) => Asset | undefined): Asset[] {
  const others = alsoIn.map((copy) => lookup(copy.id)).filter((copy): copy is Asset => !!copy && copy.id !== asset.id);
  return [asset, ...others];
}

/** `list` with the change applied (normalised, removals first so a tag can be re-added in place). */
export function changedTags(list: readonly string[], change: TagChange): string[] {
  const gone = new Set((change.remove ?? []).map((tag) => normalizeTag(tag) ?? tag));
  return normalizeTags([...list.filter((tag) => !gone.has(tag)), ...(change.add ?? [])]);
}

const sameSet = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((tag) => b.includes(tag));

/** What a change needs: the edits per library and the libraries that cannot take it. */
export interface TagPlan {
  /** Library id -> edits (qualified ids of the copies), one per copy that must change. */
  edits: Map<string, AssetMetadataEdit[]>;
  /** Library id -> why its copies cannot be changed, for those that would have had to. */
  blocked: Map<string, string>;
  /** Assets whose tags change. */
  changed: number;
}

/**
 * The plan for changing the tags of assets, each given as its copies (the shown
 * one first). Every copy that holds a different list than the new one gets an
 * edit with the full new list, in the library it lives in. A copy whose library
 * cannot be written is not edited; its library is reported in `blocked`.
 */
export function planTagChange(groups: readonly (readonly Asset[])[], change: TagChange, reason: WriteReason): TagPlan {
  const edits = new Map<string, AssetMetadataEdit[]>();
  const blocked = new Map<string, string>();
  let changed = 0;
  for (const copies of groups) {
    const next = changedTags(unionTags(copies), change);
    let touched = false;
    for (const copy of copies) {
      if (sameSet(copy.tags ?? [], next)) continue;
      const why = reason(copy.library);
      if (why) { blocked.set(copy.library, why); continue; }
      touched = true;
      const list = edits.get(copy.library);
      const edit = { id: copy.id, tags: next };
      if (list) list.push(edit); else edits.set(copy.library, [edit]);
    }
    if (touched) changed += 1;
  }
  return { edits, blocked, changed };
}

/** Whether a plan has nothing to write and nothing to report. */
export const planIsEmpty = (plan: TagPlan): boolean => plan.edits.size === 0 && plan.blocked.size === 0;

/** Whether at least one copy can be written: the tag editor is usable then. */
export const anyWritable = (copies: readonly Pick<Asset, "library">[], reason: WriteReason): boolean =>
  copies.some((copy) => reason(copy.library) === null);

/** `list` cut into runs of at most `size`. */
export function chunk<T>(list: readonly T[], size = MAX_EDITS_PER_CALL): T[][] {
  const out: T[][] = [];
  for (let start = 0; start < list.length; start += size) out.push(list.slice(start, start + size));
  return out;
}

/** What happened in one library when a plan ran. */
export interface LibraryOutcome {
  library: string;
  /** Edits saved. */
  saved: number;
  /** Why the rest was not saved (the library's own message), when it failed. */
  error?: string;
}

export interface TagResult {
  /** One plain sentence per outcome, e.g. "Saved in resources. Not saved in toybox.fornara.io: the server cannot be reached right now." */
  message: string;
  /** Something could not be saved. */
  failed: boolean;
}

const sentence = (text: string) => { const clean = text.trim().replace(/[.\s]+$/, ""); return clean ? `${clean}.` : ""; };

/** The result of running a plan, in plain words, per library. */
export function describeResult(outcomes: readonly LibraryOutcome[], blocked: ReadonlyMap<string, string>, nameOf: (library: string) => string): TagResult {
  const saved = outcomes.filter((outcome) => !outcome.error && outcome.saved > 0).map((outcome) => nameOf(outcome.library));
  const parts: string[] = [];
  if (saved.length) parts.push(`Saved in ${saved.join(", ")}.`);
  const problems: string[] = [];
  for (const outcome of outcomes) {
    if (outcome.error) problems.push(`Not saved in ${nameOf(outcome.library)}: ${sentence(outcome.error)}`);
  }
  for (const [library, why] of blocked) {
    if (!outcomes.some((outcome) => outcome.library === library)) problems.push(`Not saved in ${nameOf(library)}: ${sentence(why)}`);
  }
  parts.push(...problems);
  return { message: parts.length ? parts.join(" ") : "Nothing to change.", failed: problems.length > 0 };
}

/** Tags present on a selection (each asset counted once, however many copies it has), most common first. */
export function selectionTagCounts(groups: readonly (readonly Pick<Asset, "tags">[])[]): { tag: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const copies of groups) for (const tag of unionTags(copies)) counts.set(tag, (counts.get(tag) ?? 0) + 1);
  return [...counts].map(([tag, count]) => ({ tag, count })).sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag));
}

/** Tags typed in one field, separated by commas: the usable ones, and why any other was refused. */
export function parseTagInput(text: string): { tags: string[]; problem: string | null } {
  const parts = text.split(",").map((part) => part.trim()).filter(Boolean);
  if (parts.some((part) => part.includes(":"))) return { tags: normalizeTags(parts.filter((part) => !part.includes(":"))), problem: "A tag cannot contain a colon." };
  return { tags: normalizeTags(parts), problem: null };
}

/** What moving leftover personal tags into the library needs (used for remote libraries; a local library does it in the backend). */
export interface MigrationPlan {
  edits: Map<string, AssetMetadataEdit[]>;
  /** Asset id -> the personal tags (as stored) that become library tags once its edit is saved. */
  taken: Map<string, string[]>;
}

/**
 * Personal tags that can become library tags of their asset, in the libraries
 * `canWrite` accepts. A tag that cannot be one (colon) or does not fit (the asset
 * already has the most allowed) is left where it is. Provenance-derived tags are
 * not tags to move.
 */
export function planPersonalMigration(assets: readonly Asset[], canWrite: (library: string) => boolean): MigrationPlan {
  const edits = new Map<string, AssetMetadataEdit[]>();
  const taken = new Map<string, string[]>();
  for (const asset of assets) {
    if (!asset.user.tags.length || !canWrite(asset.library)) continue;
    const candidates = personalTags(asset).filter((tag) => normalizeTag(tag) !== null);
    if (!candidates.length) continue;
    const current = asset.tags ?? [];
    const next = normalizeTags([...current, ...candidates]);
    const moved = candidates.filter((tag) => next.includes(normalizeTag(tag)!));
    if (!moved.length) continue;
    taken.set(asset.id, asset.user.tags.filter((tag) => moved.includes(tag)));
    if (next.length !== current.length || next.some((tag, index) => tag !== current[index])) {
      const list = edits.get(asset.library);
      const edit = { id: asset.id, tags: next };
      if (list) list.push(edit); else edits.set(asset.library, [edit]);
    }
  }
  return { edits, taken };
}
