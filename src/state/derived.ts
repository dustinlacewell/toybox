/**
 * The items made from other items (`origin.derivedFrom`), as the inspector and the delete dialog show them.
 * Asked again when the contents of the libraries change; until the new answer arrives the last one stays, so
 * nothing flickers.
 */

import { useEffect, useState } from "react";

import { assetTypeOf } from "@ldlework/toybox-sdk";

import type { Asset } from "../domain/catalog";
import { derivedAssets } from "../services/browse";
import { assetTypeEntry } from "./assetTypes";
import { useStore } from "./store";

/** The items made from any of `ids` (qualified), each once; `undefined` until the first answer. A failed lookup
 *  counts as none. */
export function useDerivedAssets(ids: readonly string[]): Asset[] | undefined {
  const dataVersion = useStore((state) => state.dataVersion);
  const key = ids.join("\n");
  const [found, setFound] = useState<{ key: string; assets: Asset[] }>();
  useEffect(() => {
    if (!key) { setFound({ key, assets: [] }); return; }
    let alive = true;
    void Promise.all(key.split("\n").map((id) => derivedAssets(id).catch(() => [] as Asset[]))).then((lists) => {
      if (!alive) return;
      const byId = new Map(lists.flat().map((asset) => [asset.id, asset]));
      setFound({ key, assets: [...byId.values()] });
    });
    return () => { alive = false; };
  }, [key, dataVersion]);
  return found?.key === key ? found.assets : undefined;
}

/** "3 materials", "1 material", or "4 items" when they are of different types. */
export function countOf(assets: readonly Asset[]): string {
  const types = new Set(assets.map(assetTypeOf));
  const n = assets.length;
  if (types.size !== 1) return `${n} ${n === 1 ? "item" : "items"}`;
  const type = assetTypeEntry([...types][0]);
  return `${n} ${(n === 1 ? type.singular : type.label).toLowerCase()}`;
}
