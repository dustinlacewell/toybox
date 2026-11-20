/**
 * The question the library view asks the backend: the filter, as the arrays the backend takes, and a
 * signature that says whether two questions are the same (so a list is refetched only when it must be).
 * Pure data, no runtime imports (tests load it on its own).
 */

import type { AssetKind } from "./catalog";
import type { ExcludableField, Filter, SortOrder } from "./facets";

/** Which items are hidden because their plugin is switched off (made by `pluginVisibility`). */
export interface VisibilityRules {
  hiddenTypes: string[];
  pluginsOff: string[];
  pluginsOn: string[];
  retiredOn: string[];
  retiredOff: string[];
  ownerOff: string[];
}

export const NO_RULES: VisibilityRules = { hiddenTypes: [], pluginsOff: [], pluginsOn: [], retiredOn: [], retiredOff: [], ownerOff: [] };

/** The filter as the backend takes it (`BrowseFilter`). */
export interface BrowseFilter {
  kind: AssetKind;
  libraries: string[];
  sources: string[];
  publishers: string[];
  licenses: string[];
  formats: string[];
  categories: string[];
  tags: string[];
  favoritesOnly: boolean;
  collection: string | null;
  text: string;
  visibility: VisibilityRules;
  /** Values that take an item out, per field. */
  exclude: Record<ExcludableField, string[]>;
  /** Several tags: any one of them is enough. */
  tagsAny: boolean;
  /** Numeric properties within bounds, by path, in path order. */
  ranges: { path: string; min: number | null; max: number | null }[];
  sort: SortOrder;
}

const list = (values: ReadonlySet<string>): string[] => [...values].sort();

export function toBrowseFilter(filter: Filter, collection: string | null, visibility: VisibilityRules): BrowseFilter {
  return {
    kind: filter.kind,
    libraries: list(filter.libraries),
    sources: list(filter.sources),
    publishers: list(filter.publishers),
    licenses: list(filter.licenses),
    formats: list(filter.formats),
    categories: list(filter.categories),
    tags: list(filter.tags),
    favoritesOnly: filter.favoritesOnly,
    collection,
    text: filter.text.trim(),
    visibility,
    exclude: {
      sources: list(filter.exclude.sources),
      publishers: list(filter.exclude.publishers),
      licenses: list(filter.exclude.licenses),
      formats: list(filter.exclude.formats),
      categories: list(filter.exclude.categories),
      tags: list(filter.exclude.tags),
    },
    tagsAny: filter.tagsMatch === "any" && filter.tags.size > 1,
    ranges: Object.entries(filter.ranges)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([path, { min, max }]) => ({ path, min: min ?? null, max: max ?? null })),
    sort: { key: filter.sort.key, descending: filter.sort.descending },
  };
}

/** Two filters with the same signature ask the same question. */
export function filterSignature(filter: BrowseFilter): string {
  return JSON.stringify(filter);
}

/** The rules as one string, for deciding whether counts must be asked for again. */
export function rulesSignature(rules: VisibilityRules): string {
  return JSON.stringify(rules);
}
