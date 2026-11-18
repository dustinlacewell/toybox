/**
 * Pure catalog filtering by provenance, tags, category, favorites and text.
 */

import type { Asset, AssetKind, Category, Pack } from "./catalog";

// Kept local so this module stays free of runtime imports (it is loaded on its
// own by tests and tools). Mirrors `assetType` in ./catalog.
const assetType = (a: Asset): AssetKind => (a.kind === "plugin" ? a.type : a.kind);

export interface Filter {
  /** Which kind of asset the library shows. A mode, not a narrowing constraint. */
  kind: AssetKind;
  /** Library ids; empty means every library. */
  libraries: Set<string>;
  sources: Set<string>;
  publishers: Set<string>;
  licenses: Set<string>;
  /** Formats the assets were imported from (see `assetFormat`); "" is unknown. */
  formats: Set<string>;
  categories: Set<Category>;
  tags: Set<string>;
  favoritesOnly: boolean;
  /** Case-insensitive substring over the asset name and tags. */
  text: string;
}

export const emptyFilter = (kind: AssetKind = "model"): Filter => ({
  kind,
  libraries: new Set(),
  sources: new Set(),
  publishers: new Set(),
  licenses: new Set(),
  formats: new Set(),
  categories: new Set(),
  tags: new Set(),
  favoritesOnly: false,
  text: "",
});

export const isFilterActive = (f: Filter): boolean =>
  f.libraries.size > 0 || f.sources.size > 0 || f.publishers.size > 0 || f.licenses.size > 0 || f.formats.size > 0 ||
  f.categories.size > 0 ||
  f.tags.size > 0 ||
  f.favoritesOnly ||
  f.text.trim().length > 0;

/** The assets of one kind. */
export const ofKind = (assets: Asset[], kind: AssetKind): Asset[] =>
  assets.filter((a) => assetType(a) === kind);

/**
 * Apply a filter to the asset list: keep only assets of `f.kind`, then apply
 * the constraints. An empty facet set means "no constraint".
 */
export function applyFilter(assets: Asset[], f: Filter): Asset[] {
  const needle = f.text.trim().toLowerCase();
  return assets.filter((a) => {
    if (assetType(a) !== f.kind) return false;
    if (f.libraries.size > 0 && !f.libraries.has(a.library)) return false;
    if (f.sources.size > 0 && !f.sources.has(provenanceValue(a,"source"))) return false;
    if (f.publishers.size > 0 && !f.publishers.has(provenanceValue(a,"publisher"))) return false;
    if (f.licenses.size > 0 && !f.licenses.has(provenanceValue(a,"license"))) return false;
    if (f.formats.size > 0 && !f.formats.has(assetFormat(a))) return false;
    if (f.categories.size > 0 && !f.categories.has(a.category)) return false;
    if (f.favoritesOnly && !a.user.favorite) return false;
    if (f.tags.size) { const own = allTags(a); if (![...f.tags].every(tag=>own.includes(tag))) return false; }
    if (needle && ![a.name,...allTags(a)].some(value=>value.toLowerCase().includes(needle))) return false;
    return true;
  });
}

export interface FacetCount<T extends string> {
  value: T;
  count: number;
}

/** Count assets per library id (see `Filter.libraries`). */
export const libraryCounts = (assets: Asset[]): FacetCount<string>[] => countBy(assets, (a) => a.library);

export type ProvenanceFacet="source"|"publisher"|"license";
/** No folder fallback: absent provenance remains visibly unknown. */
export function provenanceValue(asset:Asset,field:ProvenanceFacet):string {
  return asset.user.provenance?.[field]?.trim() || asset.user.tags.find(tag=>tag.toLowerCase().startsWith(`${field}:`))?.slice(field.length+1).trim() || "";
}
export const sourceLabel=(asset:Asset)=>provenanceValue(asset,"source")||"Unknown source";
export const provenanceCounts=(assets:Asset[],field:ProvenanceFacet)=>countBy(assets,asset=>provenanceValue(asset,field));
export const isProvenanceTag=(tag:string)=>/^(source|publisher|license):/i.test(tag);

const extensionOf = (file: string | undefined): string => {
  const name = (file ?? "").split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot > 0 && dot < name.length - 1 ? name.slice(dot + 1).toLowerCase() : "";
};

/**
 * The file format an asset was imported from: the recorded `format`, else the
 * lower-case extension of its primary file, else "". Primary file per kind:
 * model/animation = the stored glTF (so "gltf" for assets imported before the
 * format was recorded), audio = its audio file, material = its origin archive
 * when it keeps one (e.g. "sbsar"), otherwise its first map, plugin = its first file.
 */
export function assetFormat(asset: Asset): string {
  const recorded = asset.format?.trim().replace(/^\.+/, "").toLowerCase();
  if (recorded) return recorded;
  switch (asset.kind) {
    case "model": case "animation": return extensionOf(asset.fileset?.gltf) || extensionOf(asset.fileName);
    case "audio": return extensionOf(asset.audio?.file) || extensionOf(asset.fileName);
    case "material": return extensionOf(asset.origin?.files?.[0]) || extensionOf(asset.material?.maps?.[0]?.file);
    case "plugin": return extensionOf(asset.files?.[0]);
    default: return "";
  }
}
export const formatCounts = (assets: Asset[]): FacetCount<string>[] => countBy(assets, assetFormat);
/** Row label for a format: upper-case, or "Unknown" when none is known. */
export const formatLabel = (format: string): string => format ? format.toUpperCase() : "Unknown";

/**
 * Every tag an asset can be found by: its tags (saved in the library)
 * followed by what this computer's cache holds in `user.tags`, each once. Provenance-derived
 * `source: ...` tags are among those and are left to the Sources, Publishers and
 * Licenses groups (see `isProvenanceTag`).
 */
export function allTags(asset:Pick<Asset,"tags"|"user">):string[] {
  return [...new Set([...(asset.tags??[]),...asset.user.tags])];
}

/** Tag rows of the Tags group: an asset's tags and the cache's own, counted once per asset. */
export function tagCounts(assets:Asset[]):FacetCount<string>[] {
  const counts=new Map<string,number>();
  for(const asset of assets)for(const tag of allTags(asset))counts.set(tag,(counts.get(tag)??0)+1);
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

/**
 * The "Imported from" line of a model or animation: the format it was imported
 * from, and whether the original file is kept with it ("FBX (original kept)").
 * Null when no format was recorded.
 */
export function importedFrom(asset: Pick<Asset, "format" | "origin">): string | null {
  const format = asset.format?.trim().replace(/^\.+/, "").toLowerCase();
  if (!format) return null;
  return asset.origin?.files?.length ? `${formatLabel(format)} (original kept)` : formatLabel(format);
}
