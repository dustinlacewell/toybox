/**
 * Application state: the loaded catalog, the active filter, the multi-select
 * selection, and the currently-previewed asset. Derived data (the filtered
 * list, facet counts) is computed from pure selectors in `domain/facets`, not
 * stored here.
 */

import { create } from "zustand";

import type { Asset, Catalog, Category, Pack, PackMeta } from "../domain/catalog";
import { emptyFilter, type Filter } from "../domain/facets";

interface AppState {
  catalog: Catalog | null;
  /** Pack identity metadata, keyed by slug — loaded from each pack's pack.json. */
  packs: Map<string, PackMeta>;
  loading: boolean;
  error: string | null;

  filter: Filter;
  /** Selected asset ids for export. */
  selection: Set<string>;
  /** Asset id open in the viewer, if any. */
  previewId: string | null;

  setCatalog: (catalog: Catalog) => void;
  setPacks: (packs: PackMeta[]) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;

  setText: (text: string) => void;
  togglePack: (pack: Pack) => void;
  toggleCategory: (category: Category) => void;
  toggleFavoritesOnly: () => void;
  clearFilter: () => void;

  toggleSelected: (id: string) => void;
  selectMany: (ids: string[]) => void;
  clearSelection: () => void;

  setPreview: (id: string | null) => void;

  /** Patch a single asset in place (favorite toggles, thumb-state updates). */
  patchAsset: (id: string, patch: Partial<Asset>) => void;
}

export const useStore = create<AppState>((set) => ({
  catalog: null,
  packs: new Map(),
  loading: false,
  error: null,
  filter: emptyFilter(),
  selection: new Set(),
  previewId: null,

  setCatalog: (catalog) => set({ catalog }),
  setPacks: (packs) => set({ packs: new Map(packs.map((p) => [p.slug, p])) }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),

  setText: (text) => set((s) => ({ filter: { ...s.filter, text } })),
  togglePack: (pack) => set((s) => ({ filter: toggleInSet(s.filter, "packs", pack) })),
  toggleCategory: (category) =>
    set((s) => ({ filter: toggleInSet(s.filter, "categories", category) })),
  toggleFavoritesOnly: () =>
    set((s) => ({ filter: { ...s.filter, favoritesOnly: !s.filter.favoritesOnly } })),
  clearFilter: () => set({ filter: emptyFilter() }),

  toggleSelected: (id) =>
    set((s) => {
      const next = new Set(s.selection);
      next.has(id) ? next.delete(id) : next.add(id);
      return { selection: next };
    }),
  selectMany: (ids) =>
    set((s) => {
      const next = new Set(s.selection);
      for (const id of ids) next.add(id);
      return { selection: next };
    }),
  clearSelection: () => set({ selection: new Set() }),

  setPreview: (previewId) => set({ previewId }),

  patchAsset: (id, patch) =>
    set((s) => {
      if (!s.catalog) return {};
      const assets = s.catalog.assets.map((a) =>
        a.id === id ? { ...a, ...patch } : a,
      );
      return { catalog: { ...s.catalog, assets } };
    }),
}));

function toggleInSet<K extends "packs" | "categories">(
  filter: Filter,
  key: K,
  value: Filter[K] extends Set<infer V> ? V : never,
): Filter {
  const next = new Set(filter[key] as Set<typeof value>);
  next.has(value) ? next.delete(value) : next.add(value);
  return { ...filter, [key]: next };
}
