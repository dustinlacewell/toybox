/**
 * Saved views: a named filter (with its order and type) kept on this computer and listed in the sidebar. A view is
 * a question, not a list: opening it shows what matches now. Pure: the filter as plain JSON and back.
 */

import { EXCLUDABLE, emptyFilter, NAME_ORDER, type Filter } from "./facets";

export interface SavedView {
  id: string;
  name: string;
  /** The filter as JSON (sets are arrays). */
  filter: StoredFilter;
  /** The collection the view was saved in, if any. */
  collection: string | null;
}

export type StoredFilter = Record<string, unknown>;

const SET_FIELDS = ["libraries", ...EXCLUDABLE] as const;

export function storeFilter(filter: Filter): StoredFilter {
  const out: StoredFilter = { kind: filter.kind, favoritesOnly: filter.favoritesOnly, text: filter.text, tagsMatch: filter.tagsMatch, ranges: filter.ranges, sort: filter.sort };
  for (const field of SET_FIELDS) out[field] = [...filter[field]].sort();
  out.exclude = Object.fromEntries(EXCLUDABLE.map((field) => [field, [...filter.exclude[field]].sort()]));
  return out;
}

const strings = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : []);
const bound = (value: unknown): number | undefined => (typeof value === "number" && Number.isFinite(value) ? value : undefined);

/** Whether a JSON value is a plain object (not null, not an array). */
export const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);

/** Kinds and sort keys an older version could store that no longer mean anything ("All assets"). */
const RETIRED_KINDS = new Set(["*"]);
const RETIRED_SORTS = new Set(["type"]);

/** A stored filter read defensively (it lives in this computer's storage and may be from an older version). Any
 *  JSON value reads as a filter; what is not an object reads as the default one. */
export function readFilter(value: unknown): Filter {
  const stored: StoredFilter = isRecord(value) ? value : {};
  const kind = typeof stored.kind === "string" && stored.kind && !RETIRED_KINDS.has(stored.kind) ? stored.kind : "model";
  const filter = emptyFilter(kind);
  for (const field of SET_FIELDS) filter[field] = new Set(strings(stored[field]));
  const exclude = isRecord(stored.exclude) ? stored.exclude : {};
  for (const field of EXCLUDABLE) filter.exclude[field] = new Set(strings(exclude[field]));
  filter.favoritesOnly = stored.favoritesOnly === true;
  filter.text = typeof stored.text === "string" ? stored.text : "";
  filter.tagsMatch = stored.tagsMatch === "any" ? "any" : "all";
  const ranges = isRecord(stored.ranges) ? stored.ranges : {};
  for (const [path, range] of Object.entries(ranges)) {
    const min = isRecord(range) ? bound(range.min) : undefined, max = isRecord(range) ? bound(range.max) : undefined;
    if (min !== undefined || max !== undefined) filter.ranges[path] = { ...(min !== undefined && { min }), ...(max !== undefined && { max }) };
  }
  const sort = isRecord(stored.sort) ? stored.sort : {};
  filter.sort = typeof sort.key === "string" && !RETIRED_SORTS.has(sort.key) ? { key: sort.key, descending: sort.descending === true } : NAME_ORDER;
  return filter;
}

/** A stored view read defensively: null when it cannot be one (no id, no name, a filter that is not an object). */
export function readView(value: unknown): SavedView | null {
  if (!isRecord(value) || typeof value.id !== "string" || typeof value.name !== "string" || !isRecord(value.filter)) return null;
  return { id: value.id, name: value.name, filter: value.filter, collection: typeof value.collection === "string" ? value.collection : null };
}

/** The stored views that can be read, from whatever the storage holds. */
export const readViews = (value: unknown): SavedView[] =>
  Array.isArray(value) ? value.map(readView).filter((view): view is SavedView => view !== null) : [];
