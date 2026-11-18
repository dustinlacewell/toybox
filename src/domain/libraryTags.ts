/**
 * Tags: descriptive keywords that belong to an asset and live in the library where
 * its file is (`Asset.tags`), so everyone who has the library sees them. There is
 * one kind of tag; `Asset.user.tags` only holds the tags derived from provenance
 * (`source: ...`) and, until they are moved, tags older versions kept on this
 * computer (see `./tagEditing`).
 *
 * Pure and free of runtime imports (tests load it on its own). `normalizeTag`
 * mirrors the backend's `domain::tags`, so a tag typed here is already in the
 * form the library will store.
 */

import type { Asset, Catalog } from "./catalog";

/** Tags kept per asset, and characters per tag; the backend cuts the excess. */
export const MAX_TAGS = 64;
export const MAX_TAG_CHARS = 48;

/** One tag in its stored form, or null if nothing usable is left: whitespace
 *  trimmed and collapsed, lower-case, no colon (that is how `source: x` provenance
 *  tags look, so a typed tag may not), cut to `MAX_TAG_CHARS`. */
export function normalizeTag(raw: string): string | null {
  if (raw.includes(":")) return null;
  const cleaned = raw.split(/\s+/).filter(Boolean).join(" ").replace(/\p{Cc}/gu, "").toLowerCase();
  const cut = [...cleaned].slice(0, MAX_TAG_CHARS).join("").trim();
  return cut || null;
}

/** A list of tags normalised: empties and duplicates dropped, order kept, at most `MAX_TAGS`. */
export function normalizeTags(raw: readonly string[]): string[] {
  const out: string[] = [];
  for (const tag of raw) {
    if (out.length >= MAX_TAGS) break;
    const next = normalizeTag(tag);
    if (next && !out.includes(next)) out.push(next);
  }
  return out;
}

/** Why a typed tag cannot be used, or null if it can. */
export function tagProblem(raw: string): string | null {
  if (!raw.trim()) return "Type a tag first.";
  if (raw.includes(":")) return "A tag cannot contain a colon.";
  return null;
}

/** `list` with `raw` added (normalised); the same list if it is empty or already there. */
export function withTag(list: readonly string[], raw: string): string[] {
  return normalizeTags([...list, raw]);
}

/** `list` without `tag`. */
export const withoutTag = (list: readonly string[], tag: string): string[] => list.filter((item) => item !== tag);

/** Tags of one copy of an asset, in order. */
export const libraryTags = (asset: Pick<Asset, "tags">): string[] => asset.tags ?? [];

/** Tags that exist only in this computer's cache, not in a library (provenance-derived `source: ...` tags are not tags to show or edit). */
export const personalTags = (asset: Pick<Asset, "user">): string[] =>
  [...new Set(asset.user.tags.filter((tag) => !/^(source|publisher|license):/i.test(tag)))];

/** A change to the library metadata of one item (`id` is the qualified id). */
export interface AssetMetadataEdit {
  id: string;
  /** Replaces the item's library tags. */
  tags?: string[];
  /** Partial: a missing field stays, null or "" clears it. */
  provenance?: { source?: string | null; publisher?: string | null; license?: string | null; url?: string | null };
}

/**
 * The catalog with `updated` (top-level assets the backend answered an edit
 * with, qualified) put in place of the same items. What only exists at run time
 * (thumbnail state) and what the edit did not touch (fingerprints) stays; with
 * nothing to replace, the same catalog comes back.
 */
export function applyUpdatedAssets(catalog: Catalog | null, updated: readonly Asset[]): Catalog | null {
  if (!catalog || !updated.length) return catalog;
  const fresh = new Map(updated.map((asset) => [asset.id, asset]));
  const keep = <T extends Asset>(next: T, old: T): T => ({
    ...next,
    thumb: old.thumb,
    contentHash: next.contentHash ?? old.contentHash,
    variants: next.variants?.map((variant) => {
      const before = old.variants?.find((candidate) => candidate.id === variant.id);
      return before ? keep(variant, before) : variant;
    }),
  });
  let changed = false;
  const assets = catalog.assets.map((asset) => {
    const next = fresh.get(asset.id);
    if (!next) return asset;
    changed = true;
    return keep(next, asset);
  });
  return changed ? { ...catalog, assets } : catalog;
}
