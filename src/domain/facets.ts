/**
 * Pure catalog filtering by provenance, tags, category, favorites and text.
 */

import type { Asset, AssetKind, Category, Pack } from "./catalog";

export interface Filter {
  /** Which kind of asset the library shows. A mode, not a narrowing constraint. */
  kind: AssetKind;
  sources: Set<string>;
  publishers: Set<string>;
  licenses: Set<string>;
  categories: Set<Category>;
  tags: Set<string>;
  favoritesOnly: boolean;
  /** Case-insensitive substring over the asset name and tags. */
  text: string;
}

export const emptyFilter = (kind: AssetKind = "model"): Filter => ({
  kind,
  sources: new Set(),
  publishers: new Set(),
  licenses: new Set(),
  categories: new Set(),
  tags: new Set(),
  favoritesOnly: false,
  text: "",
});

export const isFilterActive = (f: Filter): boolean =>
  f.sources.size > 0 || f.publishers.size > 0 || f.licenses.size > 0 ||
  f.categories.size > 0 ||
  f.tags.size > 0 ||
  f.favoritesOnly ||
  f.text.trim().length > 0;

/** The assets of one kind. */
export const ofKind = (assets: Asset[], kind: AssetKind): Asset[] =>
  assets.filter((a) => a.kind === kind);

/**
 * Apply a filter to the asset list: keep only assets of `f.kind`, then apply
 * the constraints. An empty facet set means "no constraint".
 */
export function applyFilter(assets: Asset[], f: Filter): Asset[] {
  const needle = f.text.trim().toLowerCase();
  return assets.filter((a) => {
    if (a.kind !== f.kind) return false;
    if (f.sources.size > 0 && !f.sources.has(provenanceValue(a,"source"))) return false;
    if (f.publishers.size > 0 && !f.publishers.has(provenanceValue(a,"publisher"))) return false;
    if (f.licenses.size > 0 && !f.licenses.has(provenanceValue(a,"license"))) return false;
    if (f.categories.size > 0 && !f.categories.has(a.category)) return false;
    if (f.favoritesOnly && !a.user.favorite) return false;
    if (f.tags.size && ![...f.tags].every(tag=>a.user.tags.includes(tag))) return false;
    if (needle && ![a.name,...a.user.tags].some(value=>value.toLowerCase().includes(needle))) return false;
    return true;
  });
}

export interface FacetCount<T extends string> {
  value: T;
  count: number;
}

export type ProvenanceFacet="source"|"publisher"|"license";
/** No folder fallback: absent provenance remains visibly unknown. */
export function provenanceValue(asset:Asset,field:ProvenanceFacet):string {
  return asset.user.provenance?.[field]?.trim() || asset.user.tags.find(tag=>tag.toLowerCase().startsWith(`${field}:`))?.slice(field.length+1).trim() || "";
}
export const sourceLabel=(asset:Asset)=>provenanceValue(asset,"source")||"Unknown source";
export const provenanceCounts=(assets:Asset[],field:ProvenanceFacet)=>countBy(assets,asset=>provenanceValue(asset,field));
export const isProvenanceTag=(tag:string)=>/^(source|publisher|license):/i.test(tag);

export function tagCounts(assets:Asset[]):FacetCount<string>[] {
  const counts=new Map<string,number>();
  for(const asset of assets)for(const tag of new Set(asset.user.tags))counts.set(tag,(counts.get(tag)??0)+1);
  return [...counts].map(([value,count])=>({value,count})).sort((a,b)=>a.value.localeCompare(b.value));
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
