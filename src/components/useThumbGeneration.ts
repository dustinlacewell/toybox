/**
 * Owns the thumbnail generation queue lifecycle for the library view. Builds the
 * queue from the current catalog + the pending list (from disk), exposes live
 * progress, and start/pause/regenerate controls. Reflects each asset's thumb
 * state into the store so cards update live as thumbnails land.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import type { Asset } from "../domain/catalog";
import { clearThumbs, listPendingThumbs } from "../services/tauriApi";
import { ThumbQueue, type ThumbProgress } from "../services/thumbQueue";
import { useStore } from "../state/store";

export function useThumbGeneration(assets: Asset[]) {
  const patchAsset = useStore((s) => s.patchAsset);
  const queueRef = useRef<ThumbQueue | null>(null);
  const [progress, setProgress] = useState<ThumbProgress | null>(null);

  // Tear the queue down when the view unmounts.
  useEffect(() => () => queueRef.current?.teardown(), []);

  const buildQueue = useCallback(
    (pending: string[]) => {
      const byId = new Map(assets.map((a) => [a.id, a]));
      return new ThumbQueue(byId, pending, {
        onProgress: setProgress,
        onAssetState: (id, state, error) => patchAsset(id, { thumb: { state, error } }),
      });
    },
    [assets, patchAsset],
  );

  const start = useCallback(async () => {
    if (queueRef.current?.isRunning()) return;
    if (!queueRef.current) {
      queueRef.current = buildQueue(await listPendingThumbs());
    }
    await queueRef.current.start();
  }, [buildQueue]);

  const pause = useCallback(() => queueRef.current?.pause(), []);

  /** Wipe all cached thumbnails and rebuild from scratch. */
  const regenerate = useCallback(async () => {
    queueRef.current?.teardown();
    queueRef.current = null;
    await clearThumbs();
    for (const a of assets) patchAsset(a.id, { thumb: { state: "missing" } });
    queueRef.current = buildQueue(assets.map((a) => a.id));
    await queueRef.current.start();
  }, [assets, buildQueue, patchAsset]);

  return { progress, start, pause, regenerate };
}
