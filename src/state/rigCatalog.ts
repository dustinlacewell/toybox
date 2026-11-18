/**
 * What a viewer needs to match an animation to a model: the rigs of a library and the models and animations that
 * bind to one (the old "catalog scoped to a library", without the items that have no rig). Fetched when a viewer
 * asks, once per library, and again when the library's contents changed — keeping the same object when the answer is the
 * same, so a viewer does not reload a scene for a change that did not concern it.
 */

import { useEffect, useState } from "react";

import type { Catalog } from "../domain/catalog";
import { rigCatalog } from "../services/browse";
import { useStore } from "./store";

interface Entry { version: number; value?: Catalog; signature?: string; promise?: Promise<Catalog | undefined> }
const entries = new Map<string, Entry>();

const signatureOf = (catalog: Catalog) =>
  JSON.stringify([catalog.rigs?.map((rig) => rig.id), catalog.assets.map((asset) => [asset.id, asset.kind, asset.name, asset.rigs, "fileset" in asset ? asset.fileset : null, "clip" in asset ? asset.clip : null])]);

/** The rig catalog of a library; undefined when it cannot be read. */
export function loadRigCatalog(library: string): Promise<Catalog | undefined> {
  const version = useStore.getState().dataVersion;
  const entry = entries.get(library);
  if (entry && entry.version === version && entry.promise) return entry.promise;
  const fresh: Entry = { version, value: entry?.value, signature: entry?.signature };
  fresh.promise = rigCatalog(library).then(
    (catalog) => {
      const signature = signatureOf(catalog);
      if (fresh.signature !== signature || !fresh.value) { fresh.value = catalog; fresh.signature = signature; }
      return fresh.value;
    },
    () => fresh.value,
  );
  entries.set(library, fresh);
  return fresh.promise;
}

/** The rig catalog already fetched, if any. */
export const peekRigCatalog = (library: string): Catalog | undefined => entries.get(library)?.value;

/** The rig catalog of a library for a viewer (undefined until it arrives). */
export function useRigCatalog(library: string | undefined): Catalog | undefined {
  const dataVersion = useStore((state) => state.dataVersion);
  const [value, setValue] = useState<Catalog | undefined>(library ? peekRigCatalog(library) : undefined);
  useEffect(() => {
    if (!library) { setValue(undefined); return; }
    let alive = true;
    void loadRigCatalog(library).then((catalog) => { if (alive) setValue(catalog); });
    return () => { alive = false; };
  }, [library, dataVersion]);
  return value;
}
