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
  Category,
  LibraryInfo,
  PackMeta,
  RigDefinition,
} from "../domain/catalog";
import { emptyFilter, type Filter } from "../domain/facets";
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

  /** Switch the library to another asset kind. Categories differ per kind, so they reset. */
  setKind: (kind: AssetKind) => void;
  setText: (text: string) => void;
  /** Deselect active entries; otherwise select one, or add with additive selection. */
  toggleLibrary: (library: string, additive?: boolean) => void;
  /** Rewrite the selected libraries (e.g. when one is removed from the registry). */
  setFilterLibraries: (update: (libraries: Set<string>) => Set<string>) => void;
  toggleSource: (source: string, additive?: boolean) => void;
  togglePublisher: (publisher: string, additive?: boolean) => void;
  toggleLicense: (license: string, additive?: boolean) => void;
  toggleFormat: (format: string, additive?: boolean) => void;
  toggleCategory: (category: Category, additive?: boolean) => void;
  toggleTag: (tag:string, additive?: boolean) => void;
  toggleFavoritesOnly: () => void;
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
    set((s) => ({ filter: { ...s.filter, kind, categories: new Set() }, selection: new Set(), previewId: null })),
  setText: (text) => set((s) => ({ filter: { ...s.filter, text } })),
  toggleLibrary: (library, additive) => set((s) => ({ filter: selectInSet(s.filter, "libraries", library, additive) })),
  setFilterLibraries: (update) => set((s) => ({ filter: { ...s.filter, libraries: update(s.filter.libraries) } })),
  toggleSource: (source, additive) => set((s) => ({ filter: selectInSet(s.filter, "sources", source, additive) })),
  togglePublisher: (publisher, additive) => set((s) => ({ filter: selectInSet(s.filter, "publishers", publisher, additive) })),
  toggleLicense: (license, additive) => set((s) => ({ filter: selectInSet(s.filter, "licenses", license, additive) })),
  toggleFormat: (format, additive) => set((s) => ({ filter: selectInSet(s.filter, "formats", format, additive) })),
  toggleTag: (tag, additive) => set((s) => ({ filter: selectInSet(s.filter, "tags", tag, additive) })),
  toggleCategory: (category, additive) =>
    set((s) => ({ filter: selectInSet(s.filter, "categories", category, additive) })),
  toggleFavoritesOnly: () =>
    set((s) => ({ filter: { ...s.filter, favoritesOnly: !s.filter.favoritesOnly } })),
  clearFilter: () => set((s) => ({ filter: emptyFilter(s.filter.kind) })),

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

function selectInSet<K extends "libraries" | "sources" | "publishers" | "licenses" | "formats" | "categories" | "tags">(
  filter: Filter,
  key: K,
  value: Filter[K] extends Set<infer V> ? V : never,
  additive = false,
): Filter {
  const next = new Set(filter[key] as Set<typeof value>);
  if (!additive && !next.has(value)) return { ...filter, [key]: new Set([value]) };
  next.has(value) ? next.delete(value) : next.add(value);
  return { ...filter, [key]: next };
}
