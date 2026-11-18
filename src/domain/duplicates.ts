/**
 * The same asset in several libraries, shown once.
 *
 * A copy of an item in another library is recognised by its CONTENT: two assets
 * of different libraries are the same item when they have the same type (`kind`,
 * or a plugin item's `type`) and the same `contentHash` (a fingerprint of the
 * files, independent of id, name, pack, category and file names; see
 * `toybox-core`'s `ops::content_hash`). A pair of which either side has no hash
 * yet (the library is still being fingerprinted, or the server is old) falls
 * back to type + raw id for that pair.
 *
 * Two assets of the SAME library are never merged, however alike: kits ship
 * identical models under different ids on purpose. Only copies across libraries
 * collapse.
 *
 * Libraries are walked in preference order:
 *
 *   1. a library that can be read, before one that is offline or unavailable;
 *   2. then the default library, then the other local libraries, then remote
 *      libraries (each in registry order).
 *
 * An asset is hidden when a library that comes before its own already shows a
 * matching one. When the earlier library shows several that match (identical
 * content under several ids), the hidden copy is listed under the one with its
 * own raw id if there is one, else the first.
 *
 * The same raw id in two libraries with DIFFERENT known hashes is two items, both
 * shown; `otherVersions` says so, so the interface can.
 *
 * Nothing here changes an asset or adds a field to one: what is derived is
 * returned beside the list (`alsoIn`, `otherVersions`), so what is written back
 * to a library (`splitCatalog`) can never contain anything derived. Pure data
 * functions, no runtime imports (loaded on their own by tests). A fixed number of
 * passes over the list and constant work per asset: O(n).
 */

import type { Asset, AssetKind } from "./catalog";

/** What the ranking needs of a registered library (a `LibraryInfo` fits). */
export interface LibraryRef {
  id: string;
  kind: "local" | "remote";
  isDefault?: boolean;
  available?: boolean;
  offline?: boolean;
  remote?: { offline?: boolean };
}

/** Another copy of a shown item. `id` is the copy's qualified asset id. */
export interface AlsoIn { library: string; id: string; }

export interface Deduplicated {
  /** The assets to show, in their original order. The very same objects as the input. */
  assets: Asset[];
  /** Qualified id of any copy that has others -> those other copies (all libraries, best first).
   *  Absent for an item that exists once. */
  alsoIn: Map<string, AlsoIn[]>;
  /** Qualified id -> items of other libraries with the same type and raw id whose content is
   *  known to be different (both fingerprints known and unequal). Absent for most items. */
  otherVersions: Map<string, AlsoIn[]>;
}

// Kept local so this module stays free of runtime imports. Mirrors `assetType` in ./catalog.
const assetType = (a: Asset): AssetKind => (a.kind === "plugin" ? a.type : a.kind);
const SEP = "::";
const NUL = "\u0000";
const rawId = (a: Asset): string => (a.id.startsWith(`${a.library}${SEP}`) ? a.id.slice(a.library.length + SEP.length) : a.id);
const hashOf = (a: Asset): string | undefined => a.contentHash || undefined;

const UNAVAILABLE = 1_000_000;
const REMOTE = 1_000;
const UNKNOWN = 10_000_000;

/** Preference rank per library id; lower wins. */
export function libraryRanks(libraries: readonly LibraryRef[]): Map<string, number> {
  const ranks = new Map<string, number>();
  libraries.forEach((library, index) => {
    const down = library.available === false || !!library.offline || !!library.remote?.offline;
    const base = library.isDefault ? 0 : library.kind === "remote" ? REMOTE + index : 1 + index;
    ranks.set(library.id, base + (down ? UNAVAILABLE : 0));
  });
  return ranks;
}

/** A shown asset, as the later libraries look it up. */
interface Entry { asset: Asset; lib: number; seq: number; }
/** What a later library finds under one key: the entries of the EARLIEST library that has any. */
interface Slot { lib: number; first: Entry; byRaw?: Map<string, Entry>; }

function remember(index: Map<string, Slot>, key: string, entry: Entry, raw?: string): void {
  const slot = index.get(key);
  if (!slot) {
    index.set(key, { lib: entry.lib, first: entry, byRaw: raw === undefined ? undefined : new Map([[raw, entry]]) });
  } else if (slot.lib === entry.lib && raw !== undefined && !slot.byRaw!.has(raw)) {
    slot.byRaw!.set(raw, entry);
  }
}

interface Match { entry: Entry; sameRaw: boolean; }

/** Earlier library first, then the one with the same raw id, then the first. */
const better = (a: Match, b: Match | undefined): boolean =>
  !b || a.entry.lib < b.entry.lib || (a.entry.lib === b.entry.lib && ((a.sameRaw && !b.sameRaw) || (a.sameRaw === b.sameRaw && a.entry.seq < b.entry.seq)));

interface Linked {
  /** The assets that stay visible. */
  shown: Set<Asset>;
  /** A shown asset -> everything that collapsed into it (itself first). */
  groups: Map<Asset, Asset[]>;
}

/** Walk `assets` library by library, in preference order, and link each copy to the asset that hides it. */
function link(assets: readonly Asset[], ranks: ReadonlyMap<string, number>): Linked {
  const byLibrary = new Map<string, Asset[]>();
  for (const asset of assets) {
    const list = byLibrary.get(asset.library);
    if (list) list.push(asset); else byLibrary.set(asset.library, [asset]);
  }
  const order = [...byLibrary.keys()]
    .map((id, position) => ({ id, position, rank: ranks.get(id) ?? UNKNOWN }))
    .sort((a, b) => a.rank - b.rank || a.position - b.position);

  const byHash = new Map<string, Slot>();        // type + hash -> shown assets with that hash
  const byRaw = new Map<string, Slot>();         // type + raw id -> every shown asset
  const unhashedByRaw = new Map<string, Slot>(); // type + raw id -> shown assets that have no hash yet
  const shown = new Set<Asset>();
  const groups = new Map<Asset, Asset[]>();
  let seq = 0;

  order.forEach(({ id }, lib) => {
    const added: Entry[] = [];
    for (const asset of byLibrary.get(id)!) {
      const type = assetType(asset);
      const raw = rawId(asset);
      const hash = hashOf(asset);
      const rawKey = type + NUL + raw;
      let best: Match | undefined;
      if (hash) {
        // Both hashed: the hash decides. The other side unhashed: the raw id does.
        const slot = byHash.get(type + NUL + hash);
        if (slot) {
          const sameRaw = slot.byRaw!.has(raw);
          const candidate = { entry: slot.byRaw!.get(raw) ?? slot.first, sameRaw };
          if (better(candidate, best)) best = candidate;
        }
        const open = unhashedByRaw.get(rawKey);
        if (open && better({ entry: open.first, sameRaw: true }, best)) best = { entry: open.first, sameRaw: true };
      } else {
        const any = byRaw.get(rawKey);
        if (any) best = { entry: any.first, sameRaw: true };
      }
      if (best) {
        groups.get(best.entry.asset)!.push(asset);
      } else {
        shown.add(asset);
        groups.set(asset, [asset]);
        added.push({ asset, lib, seq: seq++ });
      }
    }
    // Only now do this library's own assets become visible to the later ones.
    for (const entry of added) {
      const type = assetType(entry.asset);
      const raw = rawId(entry.asset);
      const hash = hashOf(entry.asset);
      if (hash) remember(byHash, type + NUL + hash, entry, raw);
      else remember(unhashedByRaw, type + NUL + raw, entry);
      remember(byRaw, type + NUL + raw, entry);
    }
  });
  return { shown, groups };
}

/**
 * Group `assets` and keep one copy per group. With `only` (a non-empty set of
 * library ids), only copies in those libraries are candidates, so the view shows
 * those libraries' own copies and de-duplicates among them; `alsoIn` still lists
 * the copies in every library.
 */
export function dedupeAssets(assets: readonly Asset[], ranks: ReadonlyMap<string, number>, only?: ReadonlySet<string>): Deduplicated {
  const restrict = !!only && only.size > 0;
  const everything = link(assets, ranks);
  const visible = restrict ? link(assets.filter((asset) => only!.has(asset.library)), ranks).shown : everything.shown;
  const rank = (library: string) => ranks.get(library) ?? UNKNOWN;
  const best = (a: AlsoIn, b: AlsoIn) => rank(a.library) - rank(b.library);

  const alsoIn = new Map<string, AlsoIn[]>();
  for (const members of everything.groups.values()) {
    if (members.length < 2) continue;
    // Every copy of a group with several knows the others, so the answer is the
    // same whichever copy a view happens to show.
    for (const member of members) {
      alsoIn.set(member.id, members.filter((other) => other !== member).map((other) => ({ library: other.library, id: other.id })).sort(best));
    }
  }

  // Same type and raw id in another library, both fingerprints known and different.
  const sameId = new Map<string, Asset[]>();
  for (const asset of assets) {
    const key = assetType(asset) + NUL + rawId(asset);
    const list = sameId.get(key);
    if (list) list.push(asset); else sameId.set(key, [asset]);
  }
  const otherVersions = new Map<string, AlsoIn[]>();
  for (const list of sameId.values()) {
    if (list.length < 2) continue;
    for (const asset of list) {
      const hash = hashOf(asset);
      if (!hash) continue;
      const others = list.filter((other) => other.library !== asset.library && !!hashOf(other) && hashOf(other) !== hash);
      if (others.length) otherVersions.set(asset.id, others.map((other) => ({ library: other.library, id: other.id })).sort(best));
    }
  }

  return { assets: assets.filter((asset) => visible.has(asset)), alsoIn, otherVersions };
}

/** The libraries that hold `asset`: its own, then the others. */
export const librariesHolding = (asset: Pick<Asset, "id" | "library">, alsoIn: ReadonlyMap<string, AlsoIn[]>): string[] =>
  [asset.library, ...(alsoIn.get(asset.id) ?? []).map((copy) => copy.library)];

/** Whether every one of `assets` already exists in `library` (as itself or as another copy). */
export const allThere = (assets: readonly Pick<Asset, "id" | "library">[], alsoIn: ReadonlyMap<string, AlsoIn[]>, library: string): boolean =>
  assets.length > 0 && assets.every((asset) => librariesHolding(asset, alsoIn).includes(library));
