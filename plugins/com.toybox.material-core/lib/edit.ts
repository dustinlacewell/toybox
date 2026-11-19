/**
 * The review list's edit model. Pure: each function takes the list of review items
 * and returns a new one, so the panel holds state and this file holds the rules.
 * Also the batch-level checks that need the whole list (a name used twice, an
 * unusable folder name) and the filter for what is ready to import.
 */

import type { ChannelRole, FileUse, MaterialDraft } from "./draft.js";
import { toEntry } from "./entry.js";

/** One material draft in the review list, with whether the user keeps it. */
export interface ReviewItem {
  draft: MaterialDraft;
  included: boolean;
}

type Channel = "r" | "g" | "b" | "a";

const BAD_NAME_CHARS = /[\\/:*?"<>|]/;
const EDGE_NAME = /^[ .]|[ .]$/;

function updateItem(
  items: ReviewItem[],
  index: number,
  change: (item: ReviewItem) => ReviewItem,
): ReviewItem[] {
  return items.map((item, i) => (i === index ? change(item) : item));
}

export function setIncluded(items: ReviewItem[], index: number, included: boolean): ReviewItem[] {
  return updateItem(items, index, (item) => ({ ...item, included }));
}

/** Set `included` on every item whose index is in `indexes`. */
export function setAllIncluded(
  items: ReviewItem[],
  indexes: number[],
  included: boolean,
): ReviewItem[] {
  const chosen = new Set(indexes);
  return items.map((item, i) => (chosen.has(i) ? { ...item, included } : item));
}

export function rename(items: ReviewItem[], index: number, name: string): ReviewItem[] {
  return updateItem(items, index, (item) => ({ ...item, draft: { ...item.draft, name } }));
}

export function setUse(
  items: ReviewItem[],
  index: number,
  fileIndex: number,
  use: FileUse,
): ReviewItem[] {
  return updateItem(items, index, (item) => ({
    ...item,
    draft: {
      ...item.draft,
      files: item.draft.files.map((f, i) => (i === fileIndex ? { ...f, use } : f)),
    },
  }));
}

/** Set one channel's role on a `channels` file; a file with another use is left as is. */
export function setChannel(
  items: ReviewItem[],
  index: number,
  fileIndex: number,
  channel: Channel,
  role: ChannelRole | undefined,
): ReviewItem[] {
  return updateItem(items, index, (item) => ({
    ...item,
    draft: {
      ...item.draft,
      files: item.draft.files.map((f, i) => {
        if (i !== fileIndex || f.use.use !== "channels") return f;
        return { ...f, use: { use: "channels", channels: { ...f.use.channels, [channel]: role } } };
      }),
    },
  }));
}

/** Why the item at `index` cannot be imported; empty when it can. */
export function problemsFor(
  items: ReviewItem[],
  index: number,
  pack: string,
  category: string,
): string[] {
  const item = items[index];
  const res = toEntry(item.draft, pack, category);
  const entryProblems = "problems" in res ? res.problems : [];
  return [...batchProblems(items, index), ...entryProblems];
}

function batchProblems(items: ReviewItem[], index: number): string[] {
  const name = items[index].draft.name;
  const problems: string[] = [];
  if (!isValidName(name)) problems.push("invalid name");
  if (usedByOther(items, index, name)) problems.push("name used twice");
  return problems;
}

function isValidName(name: string): boolean {
  return name.length > 0 && !BAD_NAME_CHARS.test(name) && !EDGE_NAME.test(name);
}

function usedByOther(items: ReviewItem[], index: number, name: string): boolean {
  const key = name.toLowerCase();
  return items.some((o, i) => i !== index && o.included && o.draft.name.toLowerCase() === key);
}

/** The included items with no problems — what an import would write. */
export function importable(items: ReviewItem[], pack: string, category: string): ReviewItem[] {
  return items.filter((item, i) => item.included && problemsFor(items, i, pack, category).length === 0);
}

/** Whether an import may start: something to import and a pack and category named. */
export function canImport(items: ReviewItem[], pack: string, category: string): boolean {
  return importable(items, pack, category).length > 0 && pack.trim() !== "" && category.trim() !== "";
}
