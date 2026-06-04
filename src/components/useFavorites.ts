/**
 * Favorite toggling with persistence. Updates the in-memory asset immediately
 * for snappy UI, then writes the catalog to disk. Favorites are infrequent user
 * actions, so a full catalog save per toggle is acceptable (and keeps the
 * persisted catalog authoritative for user metadata).
 */

import { useCallback } from "react";

import { saveCatalog } from "../services/tauriApi";
import { useStore } from "../state/store";

export function useFavorites() {
  const toggleFavorite = useCallback((id: string) => {
    const { catalog, patchAsset } = useStore.getState();
    const asset = catalog?.assets.find((a) => a.id === id);
    if (!catalog || !asset) return;

    patchAsset(id, { user: { ...asset.user, favorite: !asset.user.favorite } });
    // Persist from the post-patch store snapshot.
    const updated = useStore.getState().catalog;
    if (updated) void saveCatalog(updated);
  }, []);

  return { toggleFavorite };
}
