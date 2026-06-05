/**
 * Demo shim for components/useThumbUrl. Identical contract to the app's hook —
 * return the asset's thumbnail URL once its thumb is "ready", null before — but
 * without the app's `?v=<state>` cache-buster. That suffix is fine on a Tauri
 * asset:// URL (the protocol handler ignores the query), but a blob: URL can't
 * carry a query string and becomes unresolvable, so the demo reads the bare
 * object URL the thumbnail store minted for this asset.
 */

import { useEffect, useState } from "react";

import type { Asset } from "@app/domain/catalog";
import { thumbUrl } from "../shims/assetUrl";

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
      if (alive) setUrl(u);
    });
    return () => {
      alive = false;
    };
  }, [asset.id, ready]);

  return url;
}
