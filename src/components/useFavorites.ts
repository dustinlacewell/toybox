/**
 * Favorite toggling with persistence. The asset on screen changes at once for a snappy
 * interface, then only this computer's database is written (one row; the library is not touched).
 */

import { useCallback } from "react";

import { setFavorite } from "../services/browse";
import { cachedAsset, patchCachedAsset } from "../state/assetCache";
import { useStore } from "../state/store";

export function useFavorites() {
  const toggleFavorite = useCallback((id: string) => {
    const asset = cachedAsset(id);
    if (!asset) return;

    const favorite = !asset.user.favorite;
    patchCachedAsset(id, { user: { ...asset.user, favorite } });
    setFavorite([id], favorite).then(
      // A list of favourites loses the item that was just un-starred.
      () => { if (useStore.getState().filter.favoritesOnly) useStore.getState().bumpData(); },
      (failure) => {
        console.warn("Could not save the favourite", failure);
        const current = cachedAsset(id);
        if (current) patchCachedAsset(id, { user: { ...current.user, favorite: asset.user.favorite } });
      },
    );
  }, []);

  return { toggleFavorite };
}
