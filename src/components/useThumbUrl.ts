/**
 * Resolve an asset's thumbnail asset:// URL when (and only when) its thumb is
 * ready. Returns null while missing/queued/rendering so the card shows a
 * placeholder. The URL carries a cache-busting suffix keyed on thumb state so a
 * freshly-rendered thumbnail replaces a stale one.
 */

import { useEffect, useState } from "react";

import type { Asset } from "../domain/catalog";
import { thumbUrl } from "../services/assetUrl";

export function useThumbUrl(asset: Asset): string | null {
  const [url, setUrl] = useState<string | null>(null);
  const ready = asset.thumb.state === "ready";

  useEffect(() => {
    let alive = true;
    if (!ready) {
      setUrl(null);
      return;
    }
    thumbUrl(asset.id).then((u) => {
      if (alive) setUrl(`${u}?v=${asset.thumb.state}`);
    });
    return () => {
      alive = false;
    };
  }, [asset.id, ready, asset.thumb.state]);

  return url;
}
