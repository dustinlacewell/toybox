/**
 * Every asset the window has seen lately, by qualified id: what the inspector, the detail view, the
 * dialogs and the plugins read when they need one item. The list itself is paged (`state/assetList`);
 * this is where an item is found again by id, or fetched when it is not here. A patch (a favourite
 * toggled, a thumbnail finished) reaches the cache and the pages together.
 */

import { useEffect, useState, useSyncExternalStore } from "react";

import type { Asset } from "../domain/catalog";
import { unqualify } from "../domain/libraryId";
import { getAssets } from "../services/browse";
import { useStore } from "./store";

const LIMIT = 6000;
const cache = new Map<string, Asset>();
const listeners = new Set<() => void>();
const patchListeners = new Set<(change: (asset: Asset) => Asset) => void>();
let version = 0;

const emit = () => { version++; for (const listener of [...listeners]) listener(); };
export const subscribeAssets = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
const assetsVersion = () => version;

/** The pages (and anything else that keeps assets) hear of every patch to apply it to their copies. */
export const onAssetsPatched = (listener: (change: (asset: Asset) => Asset) => void) => { patchListeners.add(listener); return () => { patchListeners.delete(listener); }; };

/** Remember assets that arrived from the backend (newest last; the oldest go when there are too many). */
export function cacheAssets(assets: readonly Asset[]): void {
  for (const asset of assets) {
    cache.delete(asset.id);
    cache.set(asset.id, asset);
    for (const variant of asset.variants ?? []) { cache.delete(variant.id); cache.set(variant.id, variant); }
  }
  while (cache.size > LIMIT) cache.delete(cache.keys().next().value as string);
}

export const cachedAsset = (id: string): Asset | undefined => cache.get(id);

/** Change one asset where it is held (the cache and the pages), and tell those who show it. */
export function patchCachedAsset(id: string, patch: Partial<Pick<Asset, "thumb" | "user">>): void {
  applyPatch((asset) => (asset.id === id ? ({ ...asset, ...patch } as Asset) : asset), (held) => (held.id === id ? ({ ...held, ...patch } as Asset) : held), id);
}

/**
 * Items the backend answered an edit with, put in place of the same items wherever they are held. What only
 * exists at run time (the thumbnail state) and what the edit did not touch (the fingerprint) stays.
 */
export function replaceCachedAssets(updated: readonly Asset[]): void {
  if (!updated.length) return;
  const fresh = new Map(updated.map((asset) => [asset.id, asset]));
  const keep = (next: Asset, old: Asset): Asset => ({
    ...next,
    thumb: old.thumb,
    contentHash: next.contentHash ?? old.contentHash,
    variants: next.variants?.map((variant) => {
      const before = old.variants?.find((candidate) => candidate.id === variant.id);
      return before ? keep(variant, before) : variant;
    }),
  } as Asset);
  const change = (asset: Asset): Asset => { const next = fresh.get(asset.id); return next ? keep(next, asset) : asset; };
  for (const asset of updated) if (!cache.has(asset.id)) cache.set(asset.id, asset);
  applyPatch(change, change);
}

/** Fingerprints that arrived after the list was loaded (`{ raw id: hash }` of one library). */
export function patchCachedHashes(library: string, hashes: Readonly<Record<string, string>>): void {
  const hash = (asset: Asset) => (asset.library === library ? hashes[unqualify(library, asset.id)] : undefined);
  const change = (asset: Asset): Asset => {
    const next = hash(asset);
    const variants = asset.variants?.map(change);
    const variantsChanged = !!variants && variants.some((variant, index) => variant !== asset.variants![index]);
    if ((next === undefined || next === asset.contentHash) && !variantsChanged) return asset;
    return { ...asset, ...(next === undefined ? {} : { contentHash: next }), ...(variantsChanged ? { variants } : {}) } as Asset;
  };
  applyPatch(change, change);
}

function applyPatch(forPages: (asset: Asset) => Asset, forCache: (asset: Asset) => Asset, only?: string): void {
  let changed = false;
  if (only) {
    const held = cache.get(only);
    if (held) { const next = forCache(held); if (next !== held) { cache.set(only, next); changed = true; } }
  } else {
    for (const [id, held] of cache) { const next = forCache(held); if (next !== held) { cache.set(id, next); changed = true; } }
  }
  for (const listener of [...patchListeners]) listener(forPages);
  if (changed) emit();
}

/** Forget everything (a library left the registry). */
export function clearAssetCache(): void {
  cache.clear();
  emit();
}

const inflight = new Map<string, Promise<void>>();

/** Make sure these ids are in the cache, asking the backend for the ones that are not. */
export async function ensureAssets(ids: readonly string[], fresh = false): Promise<void> {
  const missing = ids.filter((id) => fresh || !cache.has(id));
  if (!missing.length) return;
  const wanted = missing.filter((id) => !inflight.has(id));
  if (wanted.length) {
    const job = getAssets(wanted).then((assets) => { cacheAssets(assets); emit(); }).finally(() => { for (const id of wanted) inflight.delete(id); });
    for (const id of wanted) inflight.set(id, job);
  }
  await Promise.all(missing.map((id) => inflight.get(id)));
}

/**
 * One asset by id: the cached one at once, asked for when it is not here and again when the contents of the
 * libraries change. `undefined` while it loads, and for an id that is not (or no longer) in a library.
 */
export function useAsset(id: string | null | undefined): Asset | undefined {
  useSyncExternalStore(subscribeAssets, assetsVersion);
  const dataVersion = useStore((state) => state.dataVersion);
  useEffect(() => {
    if (!id) return;
    void ensureAssets([id], cache.has(id)).catch(() => undefined);
  }, [id, dataVersion]);
  return id ? cache.get(id) : undefined;
}

/**
 * These assets (in the order of `ids`), fetched with one call and again when the libraries change. Ids that
 * are no longer there are left out; `loaded` says whether the answer to the current ids has arrived.
 */
export function useAssets(ids: readonly string[] | undefined): { assets: Asset[]; loaded: boolean } {
  const dataVersion = useStore((state) => state.dataVersion);
  const key = ids ? ids.join("\n") : null;
  const [state, setState] = useState<{ key: string | null; assets: Asset[] }>({ key: null, assets: [] });
  useEffect(() => {
    if (!ids || key === null) { setState({ key: null, assets: [] }); return; }
    let alive = true;
    getAssets(ids).then((assets) => { cacheAssets(assets); if (alive) setState({ key, assets }); }, () => { if (alive) setState({ key, assets: [] }); });
    return () => { alive = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, dataVersion]);
  return { assets: state.key === key ? state.assets : [], loaded: state.key === key };
}
