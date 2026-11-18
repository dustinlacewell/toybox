/**
 * Where else an item lives. The same item in several libraries is shown once (see `domain/duplicates` for the
 * rules and the backend's `browse` module for how the list applies them); the copies of the items on screen are
 * asked for when a panel needs them, not computed over the whole catalog.
 */

import { useEffect, useMemo, useState } from "react";

import type { Asset } from "../domain/catalog";
import { librariesHolding, type AlsoIn } from "../domain/duplicates";
import { elsewhere } from "../services/browse";
import { usePlugins } from "./plugins";
import { useStore } from "./store";

const NONE: AlsoIn[] = [];
type Copies = Map<string, { alsoIn: AlsoIn[]; otherVersions: AlsoIn[] }>;

/** The other copies of these items, by qualified id (absent: no other copy). Asked again when the libraries change. */
export function useElsewhere(ids: readonly string[]): Copies {
  const dataVersion = useStore((state) => state.dataVersion);
  const rules = usePlugins((state) => state.rules);
  const key = ids.join("\n");
  const [found, setFound] = useState<{ key: string; copies: Copies }>();
  useEffect(() => {
    if (!ids.length) { setFound({ key, copies: new Map() }); return; }
    let alive = true;
    elsewhere(ids, rules).then((copies) => { if (alive) setFound({ key, copies }); }, () => { if (alive) setFound({ key, copies: new Map() }); });
    return () => { alive = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, dataVersion, rules]);
  return useMemo(() => (found?.key === key ? found.copies : new Map()), [found, key]);
}

/** The other copies of `asset` in other libraries (empty for most items). */
export function useAlsoIn(asset: Pick<Asset, "id" | "library">): AlsoIn[] {
  const copies = useElsewhere(useMemo(() => [asset.id], [asset.id]));
  return copies.get(asset.id)?.alsoIn ?? NONE;
}

/** Copies of `asset` in other libraries whose content is known to differ (same id, another version). */
export function useOtherVersions(asset: Pick<Asset, "id" | "library">): AlsoIn[] {
  const copies = useElsewhere(useMemo(() => [asset.id], [asset.id]));
  return copies.get(asset.id)?.otherVersions ?? NONE;
}

/** `{ id: other copies }` for the dialogs that list where an item also lives. */
export function alsoInMap(copies: Copies): ReadonlyMap<string, AlsoIn[]> {
  return new Map([...copies].map(([id, entry]) => [id, entry.alsoIn]));
}

export { librariesHolding };
