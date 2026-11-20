/**
 * Application state: the registered libraries, the active filter, the multi-select
 * selection, and the currently-previewed asset. The window holds no catalog: the
 * visible list, counts and single assets are queries (`services/browse`, `state/assetList`,
 * `state/assetCache`, `state/libraryCounts`), asked again when `dataVersion` moves.
 */

import { create } from "zustand";

import type {
  Asset,
  AssetKind,
  LibraryInfo,
  PackMeta,
  RigDefinition,
} from "../domain/catalog";
import { emptyFilter, withFacet, NAME_ORDER, type FacetField, type Filter, type SortOrder } from "../domain/facets";
import { applyQuery, parseQuery } from "../domain/query";
import { patchCachedAsset, patchCachedHashes } from "./assetCache";

/** The fields the host updates in place; common to every asset kind. */
type AssetPatch = Partial<Pick<Asset, "thumb" | "user">>;

interface AppState {
  /** Moves up whenever the contents of a library changed (an import, a delete, a tag edit, a rescan, fingerprints):
   *  the list, the counts and every asset on screen are asked for again. Qualified ids stay valid across it. */
  dataVersion: number;
  /** Items each library holds, by library id (as of the last time it was made ready or changed). */
  libraryCounts: Record<string, number>;
  setLibraryCount: (library: string, count: number) => void;
  /** The rigs of every library (a rig id is a hash of its contents, so equal ids are the same rig). */
  rigs: RigDefinition[];
  /** The registered libraries, in the order they were added. */
  libraries: LibraryInfo[];
  /** Why a library could not be read, by library id. */
  libraryErrors: Record<string, string>;
  /** Pack identity metadata, keyed by slug — loaded from each pack's pack.json. */
  packs: Map<string, PackMeta>;
  loading: boolean;
  error: string | null;
  /** Libraries whose items are being fingerprinted for duplicate detection (first time only), by library id. */
  fingerprinting: Record<string, { done: number; total: number }>;

  filter: Filter;
  /** Selected asset ids for export. */
  selection: Set<string>;
  /** Asset id open in the viewer, if any. */
  previewId: string | null;

  /** Note that library contents changed. */
  bumpData: () => void;
  /** `bumpData`, but at most once in a while (fingerprints arrive in a stream). */
  bumpDataSoon: () => void;
  setRigs: (rigs: RigDefinition[]) => void;
  setLibraries: (libraries: LibraryInfo[]) => void;
  setLibraryErrors: (errors: Record<string, string>) => void;
  setPacks: (packs: PackMeta[]) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  /** Progress of the background fingerprinting of one library (`null`: it is done). */
  setFingerprinting: (library: string, progress: { done: number; total: number } | null) => void;
  /** Fingerprints computed after the list was loaded: `{ raw asset id: hash }` of one library. */
  applyContentHashes: (library: string, hashes: Record<string, string>) => void;

  /** Switch the library to another asset kind (or every type). Categories and properties differ per kind, so they reset. */
  setKind: (kind: AssetKind) => void;
  setText: (text: string) => void;
  /** Take the field terms out of typed query text into the filter; the rest becomes the search text. */
  applyQueryText: (text: string) => void;
  /** Choose a value of a field, leave it out, or neither; `only` makes it the field's one chosen value. */
  setFacet: (field: FacetField, value: string, state: "in" | "out" | "off", only?: boolean) => void;
  /** Rewrite the selected libraries (e.g. when one is removed from the registry). */
  setFilterLibraries: (update: (libraries: Set<string>) => Set<string>) => void;
  setTagsMatch: (match: Filter["tagsMatch"]) => void;
  setSort: (sort: SortOrder) => void;
  /** Bounds of a numeric property (`null`: no condition on it). */
  setRange: (path: string, bounds: { min?: number; max?: number } | null) => void;
  toggleFavoritesOnly: () => void;
  /** Replace the whole filter (a saved view). */
  setFilter: (filter: Filter) => void;
  /** Every condition off; the type and the order stay. */
  clearFilter: () => void;

  toggleSelected: (id: string) => void;
  selectMany: (ids: string[]) => void;
  setSelection: (ids: string[]) => void;
  clearSelection: () => void;

  setPreview: (id: string | null) => void;

  /** Patch a single asset in place (favorite toggles, thumb-state updates). */
  patchAsset: (id: string, patch: AssetPatch) => void;
}

let soon: ReturnType<typeof setTimeout> | undefined;

export const useStore = create<AppState>((set, get) => ({
  dataVersion: 0,
  libraryCounts: {},
  setLibraryCount: (library, count) => set((s) => (s.libraryCounts[library] === count ? {} : { libraryCounts: { ...s.libraryCounts, [library]: count } })),
  rigs: [],
  libraries: [],
  libraryErrors: {},
  packs: new Map(),
  loading: false,
  error: null,
  fingerprinting: {},
  filter: emptyFilter(),
  selection: new Set(),
  previewId: null,

  bumpData: () => set((s) => ({ dataVersion: s.dataVersion + 1 })),
  bumpDataSoon: () => {
    if (soon) return;
    soon = setTimeout(() => { soon = undefined; get().bumpData(); }, 2000);
  },
  setRigs: (rigs) => set({ rigs }),
  setLibraries: (libraries) => set({ libraries }),
  setLibraryErrors: (libraryErrors) => set({ libraryErrors }),
  setPacks: (packs) => set({ packs: new Map(packs.map((p) => [p.slug, p])) }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
  setFingerprinting: (library, progress) =>
    set((s) => {
      const { [library]: _previous, ...rest } = s.fingerprinting;
      return { fingerprinting: progress ? { ...rest, [library]: progress } : rest };
    }),
  applyContentHashes: (library, hashes) => {
    // The ones on screen show it at once; the list and the counts follow (duplicates may now be hidden).
    patchCachedHashes(library, hashes);
    get().bumpDataSoon();
  },

  setKind: (kind) =>
    set((s) => ({
      filter: {
        ...s.filter,
        kind,
        categories: new Set(),
        exclude: { ...s.filter.exclude, categories: new Set() },
        ranges: {},
        sort: s.filter.sort.key.startsWith("prop:") ? NAME_ORDER : s.filter.sort,
      },
      selection: new Set(),
      previewId: null,
    })),
  setText: (text) => set((s) => ({ filter: { ...s.filter, text } })),
  applyQueryText: (text) => set((s) => ({ filter: applyQuery(s.filter, parseQuery(text)) })),
  setFacet: (field, value, state, only) => set((s) => ({ filter: withFacet(s.filter, field, value, state, only) })),
  setFilterLibraries: (update) => set((s) => ({ filter: { ...s.filter, libraries: update(s.filter.libraries) } })),
  setTagsMatch: (tagsMatch) => set((s) => ({ filter: { ...s.filter, tagsMatch } })),
  setSort: (sort) => set((s) => ({ filter: { ...s.filter, sort } })),
  setRange: (path, bounds) =>
    set((s) => {
      const ranges = { ...s.filter.ranges };
      if (bounds && (bounds.min !== undefined || bounds.max !== undefined)) ranges[path] = bounds;
      else delete ranges[path];
      return { filter: { ...s.filter, ranges } };
    }),
  toggleFavoritesOnly: () =>
    set((s) => ({ filter: { ...s.filter, favoritesOnly: !s.filter.favoritesOnly } })),
  setFilter: (filter) => set({ filter, selection: new Set(), previewId: null }),
  clearFilter: () => set((s) => ({ filter: { ...emptyFilter(s.filter.kind), sort: s.filter.sort } })),

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
  setSelection: (ids) => set({ selection: new Set(ids) }),

  setPreview: (previewId) => set({ previewId }),

  patchAsset: (id, patch) => patchCachedAsset(id, patch),
}));

