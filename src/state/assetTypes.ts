/**
 * The registered asset types, shared by every component that lists, labels or
 * renders by type. Seeded with the native defaults and replaced once plugin
 * discovery finishes.
 */

import { create } from "zustand";

import type { AssetKind } from "../domain/catalog";
import { defaultAssetTypes, unknownAssetType, type AssetTypeEntry } from "../services/assetTypes";

interface AssetTypesState {
  types: AssetTypeEntry[];
  setTypes(types: AssetTypeEntry[]): void;
}

export const useAssetTypes = create<AssetTypesState>((set) => ({
  types: defaultAssetTypes(),
  setTypes: (types) => set({ types }),
}));

let markLoaded!: () => void;
const loaded = new Promise<void>((resolve) => { markLoaded = resolve; });

/** Resolves once plugin discovery has finished (or failed). Work that needs a
 *  plugin's implementation — rendering a thumbnail, say — waits for this, so a
 *  library that loads before its plugins is not mistaken for one without them. */
export const assetTypesLoaded = (): Promise<void> => loaded;
export const markAssetTypesLoaded = (): void => markLoaded();

/** The entry for `id`; a stand-in when its plugin is not installed. */
export function assetTypeEntry(id: AssetKind, types: AssetTypeEntry[] = useAssetTypes.getState().types): AssetTypeEntry {
  return types.find((entry) => entry.id === id) ?? unknownAssetType(id);
}

export function useAssetType(id: AssetKind): AssetTypeEntry {
  const types = useAssetTypes((state) => state.types);
  return assetTypeEntry(id, types);
}
