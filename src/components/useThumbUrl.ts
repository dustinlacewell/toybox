import { useEffect, useState } from "react";
import type { Asset } from "../domain/catalog";
import { thumbUrl } from "../services/assetUrl";

// Virtualized cards can remount while a replacement is being generated.
const displayed = new Map<string, { url: string; revision: number }>();

/** Keep the previous picture until its replacement has finished loading. */
export function useThumbUrl(asset: Asset): string | null {
  const [picture, setPicture] = useState<{ id: string; url: string } | null>(null);
  const available = asset.thumb.state === "ready" || !!asset.thumb.available;
  const revision = asset.thumb.revision ?? 0;
  useEffect(() => {
    if (!available) return;
    let alive = true;
    const cached = displayed.get(asset.id);
    if (cached?.revision === revision) {
      setPicture({ id: asset.id, url: cached.url });
      return;
    }
    void thumbUrl(asset.id).then(async url => {
      const next = `${url}?v=${revision}-${Date.now()}`;
      const image = new Image(); image.src = next;
      await image.decode();
      if (alive) {
        displayed.set(asset.id, { url: next, revision });
        setPicture({ id: asset.id, url: next });
      }
    }).catch(() => { /* A failed replacement leaves the current picture visible. */ });
    return () => { alive = false; };
  }, [asset.id, available, revision]);
  return picture?.id === asset.id ? picture.url : displayed.get(asset.id)?.url ?? null;
}
