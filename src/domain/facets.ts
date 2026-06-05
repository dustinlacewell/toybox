/**
 * Pure facet selection over the catalog: filtering by pack/category/favorite/
 * text and computing per-facet counts for filter badges. No side effects.
 */

import type { Asset, Category, Pack } from "./catalog";

export interface Filter {
  packs: Set<Pack>;
  categories: Set<Category>;
  favoritesOnly: boolean;
  /** Case-insensitive substring over the asset name. */
  text: string;
}

export const emptyFilter = (): Filter => ({
  packs: new Set(),
  categories: new Set(),
  favoritesOnly: false,
  text: "",
});

export const isFilterActive = (f: Filter): boolean =>
  f.packs.size > 0 ||
  f.categories.size > 0 ||
  f.favoritesOnly ||
  f.text.trim().length > 0;

/** Apply a filter to the asset list. An empty facet set means "no constraint". */
export function applyFilter(assets: Asset[], f: Filter): Asset[] {
  const needle = f.text.trim().toLowerCase();
  return assets.filter((a) => {
    if (f.packs.size > 0 && !f.packs.has(a.pack)) return false;
    if (f.categories.size > 0 && !f.categories.has(a.category)) return false;
    if (f.favoritesOnly && !a.user.favorite) return false;
    if (needle && !a.name.toLowerCase().includes(needle)) return false;
    return true;
  });
}

export interface FacetCount<T extends string> {
  value: T;
  count: number;
}

/** Count assets per pack across the full (unfiltered) set, sorted by value. */
export function packCounts(assets: Asset[]): FacetCount<Pack>[] {
  return countBy(assets, (a) => a.pack);
}

/** Count assets per category across the full set, sorted by value. */
export function categoryCounts(assets: Asset[]): FacetCount<Category>[] {
  return countBy(assets, (a) => a.category);
}

function countBy<T extends string>(
  assets: Asset[],
  key: (a: Asset) => T,
): FacetCount<T>[] {
  const map = new Map<T, number>();
  for (const a of assets) {
    const k = key(a);
    map.set(k, (map.get(k) ?? 0) + 1);
  }
  return [...map.entries()]
    .map(([value, count]) => ({ value, count }))
    .sort((a, b) => a.value.localeCompare(b.value));
}
