/**
 * Library identity at the API boundary. Pure data functions, no runtime imports
 * (loaded on their own by tests).
 *
 * A raw asset id is unique only inside its library. The frontend holds one
 * merged catalog, so every asset id it sees is QUALIFIED: `"<libraryId>::<rawId>"`.
 * Qualifying happens when a catalog arrives from the backend and un-qualifying
 * when anything is sent back (see `services/tauriApi.ts`), so nothing past that
 * boundary, and nothing written to a library manifest or a per-library cache,
 * ever has the other form.
 *
 * Library ids are `[A-Za-z0-9_-]+` and never contain ':', so the FIRST "::" always
 * ends the library id, whatever the raw id holds. Ids that are not asset ids
 * (the built-in preview sentinels, which start with "toybox:") are never qualified.
 */

import type { Asset, Catalog, LibraryCatalog, RigDefinition } from "./catalog";

export const SEP = "::";
/** Built-in references such as the mannequin and skeleton previews. */
const BUILTIN_PREFIX = "toybox:";

export const qualify = (library: string, id: string): string => `${library}${SEP}${id}`;

/** The library and raw id of a qualified id; null when `id` is not qualified. */
export function parseQualified(id: string): { library: string; id: string } | null {
  const at = id.indexOf(SEP);
  if (at <= 0) return null;
  const library = id.slice(0, at);
  return /^[A-Za-z0-9_-]+$/.test(library) ? { library, id: id.slice(at + SEP.length) } : null;
}

/** The library a qualified id belongs to; throws for an id that is not qualified. */
export function libraryOf(id: string): string {
  const parsed = parseQualified(id);
  if (!parsed) throw new Error(`Not a library item id: ${id}`);
  return parsed.library;
}

/** The raw id of `id` inside `library`. A reference that is not qualified with
 *  that library (a built-in sentinel, an already-raw id) comes back unchanged. */
export function unqualify(library: string, id: string): string {
  return id.startsWith(`${library}${SEP}`) ? id.slice(library.length + SEP.length) : id;
}

const qualifyRef = (library: string, id: string): string =>
  id.startsWith(BUILTIN_PREFIX) || id.startsWith(`${library}${SEP}`) ? id : qualify(library, id);

/** The same asset with its id and every reference to another asset qualified,
 *  stamped with `library`; variants included. References: `id`,
 *  `user.previewAssetId`, an animation's `clip.sourceId`, `origin.derivedFrom`,
 *  and the same on variants. */
export function qualifyAsset<T extends Asset>(asset: T, library: string): T {
  const out: Record<string, unknown> = { ...asset, id: qualify(library, asset.id), library };
  if (asset.user.previewAssetId) out.user = { ...asset.user, previewAssetId: qualifyRef(library, asset.user.previewAssetId) };
  if (asset.kind === "animation" && asset.clip?.sourceId) out.clip = { ...asset.clip, sourceId: qualifyRef(library, asset.clip.sourceId) };
  if (asset.origin?.derivedFrom) out.origin = { ...asset.origin, derivedFrom: qualifyRef(library, asset.origin.derivedFrom) };
  if (asset.variants) out.variants = asset.variants.map((variant) => qualifyAsset(variant, library));
  return out as unknown as T;
}

/** Inverse of `qualifyAsset`: raw ids and references, no `library` stamp. */
export function unqualifyAsset<T extends Asset>(asset: T, library: string): T {
  const { library: _stamp, ...rest } = asset as T & { library?: string };
  const out: Record<string, unknown> = { ...rest, id: unqualify(library, asset.id) };
  if (asset.user.previewAssetId) out.user = { ...asset.user, previewAssetId: unqualify(library, asset.user.previewAssetId) };
  if (asset.kind === "animation" && asset.clip?.sourceId) out.clip = { ...asset.clip, sourceId: unqualify(library, asset.clip.sourceId) };
  if (asset.origin?.derivedFrom) out.origin = { ...asset.origin, derivedFrom: unqualify(library, asset.origin.derivedFrom) };
  if (asset.variants) out.variants = asset.variants.map((variant) => unqualifyAsset(variant, library));
  return out as unknown as T;
}

/** One library's catalog as the backend sent it, qualified. The library root is
 *  dropped: a merged catalog has no single root; assets carry `library` instead. */
export function qualifyCatalog(raw: LibraryCatalog, library: string): Catalog {
  const { libraryRoot: _root, ...rest } = raw;
  return { ...rest, assets: raw.assets.map((asset) => qualifyAsset(asset, library)) };
}

/** The merged catalog: every library's assets in registry order, rigs unioned
 *  (a rig id is a hash of its contents, so equal ids are the same rig). */
export function mergeCatalogs(parts: { library: string; catalog: Catalog }[]): Catalog {
  const rigs = new Map<string, RigDefinition>();
  for (const { catalog } of parts) for (const rig of catalog.rigs ?? []) if (!rigs.has(rig.id)) rigs.set(rig.id, rig);
  const versions = parts.map(({ catalog }) => catalog.schemaVersion);
  return {
    schemaVersion: versions.length ? Math.min(...versions) : 0,
    rigs: [...rigs.values()],
    assets: parts.flatMap(({ catalog }) => catalog.assets),
  };
}

/** Replace one library's slice of the merged catalog with a fresh one, keeping
 *  the other libraries as they are and the assets in `order` of libraries. */
export function replaceLibrary(merged: Catalog | null, library: string, fresh: Catalog, order: string[]): Catalog {
  const byLibrary = new Map<string, Asset[]>();
  for (const asset of merged?.assets ?? []) {
    if (asset.library === library) continue;
    byLibrary.set(asset.library, [...(byLibrary.get(asset.library) ?? []), asset]);
  }
  byLibrary.set(library, fresh.assets);
  const rigs = new Map<string, RigDefinition>();
  for (const rig of [...(merged?.rigs ?? []), ...(fresh.rigs ?? [])]) if (!rigs.has(rig.id)) rigs.set(rig.id, rig);
  const known = new Set(order);
  const sequence = [...order, ...[...byLibrary.keys()].filter((id) => !known.has(id))];
  return { schemaVersion: fresh.schemaVersion, rigs: [...rigs.values()], assets: sequence.flatMap((id) => byLibrary.get(id) ?? []) };
}

/** Content fingerprints that arrived after the catalog (`{ raw asset id: hash }`, variants
 *  included) set on that library's items. Only changed items are copied; with nothing to
 *  change, the same catalog comes back, so nothing downstream recomputes for no reason. */
export function patchContentHashes(catalog: Catalog | null, library: string, hashes: Readonly<Record<string, string>>): Catalog | null {
  if (!catalog) return catalog;
  const patch = <T extends Asset>(asset: T): T => {
    const hash = Object.prototype.hasOwnProperty.call(hashes, unqualify(library, asset.id)) ? hashes[unqualify(library, asset.id)] : undefined;
    const variants = asset.variants?.map(patch);
    const variantsChanged = !!variants && variants.some((variant, index) => variant !== asset.variants![index]);
    if ((hash === undefined || hash === asset.contentHash) && !variantsChanged) return asset;
    return { ...asset, ...(hash === undefined ? {} : { contentHash: hash }), ...(variantsChanged ? { variants } : {}) };
  };
  let changed = false;
  const assets = catalog.assets.map((asset) => {
    if (asset.library !== library) return asset;
    const next = patch(asset);
    if (next !== asset) changed = true;
    return next;
  });
  return changed ? { ...catalog, assets } : catalog;
}

/** The merged catalog without a library (when it is removed from the registry). */
export function dropLibrary(merged: Catalog, library: string): Catalog {
  return { ...merged, assets: merged.assets.filter((asset) => asset.library !== library) };
}

const scopes = new WeakMap<Catalog, Map<string, Catalog>>();
/** The merged catalog narrowed to one library. Previews, retargeting and rig
 *  matching read files, and a read happens inside a single library, so they only
 *  ever consider that library's items. Memoised so the result is referentially
 *  stable for as long as the merged catalog is. */
export function scopeCatalog<C extends Catalog | null | undefined>(catalog: C, library: string): C {
  if (!catalog) return catalog;
  let byLibrary = scopes.get(catalog);
  if (!byLibrary) scopes.set(catalog, (byLibrary = new Map()));
  let scoped = byLibrary.get(library);
  if (!scoped) byLibrary.set(library, (scoped = { ...catalog, assets: catalog.assets.filter((asset) => asset.library === library) }));
  return scoped as C;
}

/** Split the merged catalog back into one raw catalog per library, to be saved.
 *  Only libraries that have assets are written: a library whose catalog failed
 *  to load has none here, and an empty write would wipe its personal metadata. */
export function splitCatalog(merged: Catalog): Map<string, LibraryCatalog> {
  const out = new Map<string, LibraryCatalog>();
  for (const asset of merged.assets) {
    const part = out.get(asset.library) ?? { schemaVersion: merged.schemaVersion, libraryRoot: "", rigs: [], assets: [] };
    part.assets.push(unqualifyAsset(asset, asset.library));
    out.set(asset.library, part);
  }
  return out;
}

/** Group qualified ids by library, as raw ids, in first-seen library order. */
export function groupIds(ids: readonly string[]): Map<string, string[]> {
  const groups = new Map<string, string[]>();
  for (const id of ids) {
    const parsed = parseQualified(id);
    if (!parsed) throw new Error(`Not a library item id: ${id}`);
    groups.set(parsed.library, [...(groups.get(parsed.library) ?? []), parsed.id]);
  }
  return groups;
}

type EntryOrigin = { derivedFrom?: string };
type EntryRefs = { id: string; origin?: EntryOrigin; variants?: { id: string; origin?: EntryOrigin }[] };

/** A seed entry a plugin built for `library` with raw ids. A plugin may reuse the
 *  qualified `asset.id` it was handed to replace that item in place, and name the
 *  item an entry was made from (`origin.derivedFrom`) by the qualified id it was
 *  handed; the host strips the qualifier so only raw ids reach the library. */
export function unqualifyEntry<E extends EntryRefs>(library: string, entry: E): E {
  const refs = <T extends { id: string; origin?: EntryOrigin }>(item: T): T => {
    const out = { ...item, id: unqualify(library, item.id) };
    if (item.origin?.derivedFrom) out.origin = { ...item.origin, derivedFrom: unqualify(library, item.origin.derivedFrom) };
    return out;
  };
  const out = refs(entry);
  if (entry.variants) out.variants = entry.variants.map(refs);
  return out;
}

/** Qualify a list or a keyed record coming back from a per-library call. */
export const qualifyAll = (library: string, ids: readonly string[]): string[] => ids.map((id) => qualify(library, id));
export function qualifyKeys<T>(library: string, record: Record<string, T>): Record<string, T> {
  return Object.fromEntries(Object.entries(record).map(([id, value]) => [qualify(library, id), value]));
}

/** The library saved exports and a drawer opened for `ids` belong to: the first
 *  item's library, else `fallback`. */
export function scopeLibrary(ids: readonly string[], fallback: string | null): string | null {
  for (const id of ids) { const parsed = parseQualified(id); if (parsed) return parsed.library; }
  return fallback;
}

/** A merged export report. When two libraries wrote the same target-relative
 *  file, the later one replaced the earlier; say so. */
export function mergeReports<R extends { written: string[]; skipped: string[]; warnings: string[] }>(reports: R[]): R {
  const merged = { written: [] as string[], skipped: [] as string[], warnings: [] as string[] } as R;
  const seen = new Map<string, number>();
  let clashes = 0;
  reports.forEach((report, index) => {
    for (const path of report.written) {
      const previous = seen.get(path);
      if (previous !== undefined && previous !== index) clashes++;
      seen.set(path, index);
      merged.written.push(path);
    }
    merged.skipped.push(...report.skipped);
    merged.warnings.push(...report.warnings);
  });
  if (clashes) merged.warnings.push(`${clashes} file${clashes === 1 ? " was" : "s were"} written by more than one library to the same path, so the later one replaced the earlier. Export one library at a time to keep them apart.`);
  return merged;
}
