/**
 * The question the library view asks the backend: the filter, as the arrays the backend takes, and a
 * signature that says whether two questions are the same (so a list is refetched only when it must be).
 * Pure data, no runtime imports (tests load it on its own).
 */

import type { AssetKind } from "./catalog";
import type { Filter } from "./facets";

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
