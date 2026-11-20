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

/** A stored filter read defensively (it lives in this computer's storage and may be from an older version). */
export function readFilter(stored: StoredFilter): Filter {
  const filter = emptyFilter(typeof stored.kind === "string" && stored.kind ? stored.kind : "model");
  for (const field of SET_FIELDS) filter[field] = new Set(strings(stored[field]));
  const exclude = (stored.exclude ?? {}) as Record<string, unknown>;
  for (const field of EXCLUDABLE) filter.exclude[field] = new Set(strings(exclude[field]));
  filter.favoritesOnly = stored.favoritesOnly === true;
  filter.text = typeof stored.text === "string" ? stored.text : "";
  filter.tagsMatch = stored.tagsMatch === "any" ? "any" : "all";
  const ranges = (stored.ranges ?? {}) as Record<string, { min?: unknown; max?: unknown }>;
  for (const [path, value] of Object.entries(ranges)) {
    const min = bound(value?.min), max = bound(value?.max);
    if (min !== undefined || max !== undefined) filter.ranges[path] = { ...(min !== undefined && { min }), ...(max !== undefined && { max }) };
  }
  const sort = stored.sort as { key?: unknown; descending?: unknown } | undefined;
  filter.sort = typeof sort?.key === "string" ? { key: sort.key, descending: sort.descending === true } : NAME_ORDER;
  return filter;
}
