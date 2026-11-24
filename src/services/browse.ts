/**
 * What the library view asks of the backend: pages of the list, counts, items by id and the small writes of
 * personal data. The window holds no catalog; every one of these is a query (see
 * `docs/sqlite-storage.md`, "App queries"). Ids that come back are qualified (see `domain/libraryId`) and ids
 * that go out are grouped by library and sent raw, like everything in `tauriApi`.
 */

import { invoke } from "@tauri-apps/api/core";

import type { Asset, Catalog, RigDefinition } from "../domain/catalog";
import type { BrowseFilter, VisibilityRules } from "../domain/browseQuery";
import { collectDerived } from "../domain/derived";
import type { AlsoIn } from "../domain/duplicates";
import { groupIds, parseQualified, qualify, qualifyAsset } from "../domain/libraryId";
import { usePlugins } from "../state/plugins";

/** Where a library stands once it is ready to be read. */
export interface LibrarySummary { library: string; count: number; revision: number }

interface Row { library: string; asset: Asset }
const asset = (row: Row): Asset => qualifyAsset(row.asset, row.library);

/** Make a library ready to be read (migrate it, build its catalog, bring a remote one up to date). */
export const openLibrary = (library: string, force: boolean): Promise<LibrarySummary> => invoke("open_library", { library, force });

/** A page of the list the filter shows, and how long the list is. */
export async function queryAssets(filter: BrowseFilter, offset: number, limit: number): Promise<{ total: number; exact: boolean; assets: Asset[] }> {
  const page = await invoke<{ total: number; exact: boolean; rows: Row[] }>("query_assets", { filter, descending: false, offset, limit });
  return { total: page.total, exact: page.exact, assets: page.rows.map(asset) };
}

/** The items (any type) made from the item `parentId` (qualified): those whose `origin.derivedFrom` is it, from
 *  its library, hidden the way the library view hides them (a switched-off plugin's items stay out). Every
 *  library, a remote one included (read through its local replica), applies the filter; the answer is checked
 *  here as well, and all of its pages are read. */
export async function derivedAssets(parentId: string): Promise<Asset[]> {
  const parent = parseQualified(parentId);
  if (!parent) throw new Error(`Not a library item id: ${parentId}`);
  const filter: BrowseFilter = {
    kind: "", libraries: [parent.library], sources: [], publishers: [], licenses: [], formats: [], categories: [], tags: [],
    favoritesOnly: false, collection: null, text: "", visibility: usePlugins.getState().rules, derivedFrom: parent.id,
    exclude: { sources: [], publishers: [], licenses: [], formats: [], categories: [], tags: [] }, tagsAny: false, ranges: [],
    sort: { key: "name", descending: false },
  };
  return collectDerived(async (offset, limit) => (await queryAssets(filter, offset, limit)).assets, parentId, DERIVED_PAGE);
}

const DERIVED_PAGE = 500;

/** How many items the filter shows, exactly (for a page whose `exact` was false). */
export const countAssets = (filter: BrowseFilter): Promise<number> => invoke("count_assets", { filter });

/** The ids (qualified) of the items of the list from `offset` on, at most `cap`; `truncated` when there are more. */
export async function queryIds(filter: BrowseFilter, offset: number, cap: number): Promise<{ ids: string[]; truncated: boolean }> {
  const page = await invoke<{ ids: { library: string; id: string }[]; truncated: boolean }>("query_ids", { filter, descending: false, offset, cap });
  return { ids: page.ids.map((item) => qualify(item.library, item.id)), truncated: page.truncated };
}

/** Items or variants by qualified id, in the order asked; ids that are not there are left out. */
export async function getAssets(ids: readonly string[]): Promise<Asset[]> {
  if (!ids.length) return [];
  const items = ids.map((id) => {
    const parsed = parseQualified(id);
    if (!parsed) throw new Error(`Not a library item id: ${id}`);
    return { library: parsed.library, id: parsed.id };
  });
  // A selection of tens of thousands is asked for in pieces, so no single reply (or request) is a hundred megabytes.
  const pieces: typeof items[] = [];
  for (let at = 0; at < items.length; at += GET_CHUNK) pieces.push(items.slice(at, at + GET_CHUNK));
  const rows: Row[] = [];
  for (let at = 0; at < pieces.length; at += GET_PARALLEL) {
    const answers = await Promise.all(pieces.slice(at, at + GET_PARALLEL).map((piece) => invoke<Row[]>("get_assets", { items: piece })));
    for (const answer of answers) rows.push(...answer);
  }
  return rows.map(asset);
}

/** Items asked for in one `get_assets`, and how many such requests are on their way at once. */
const GET_CHUNK = 2000;
const GET_PARALLEL = 3;

export interface Count { value: string; count: number }

/** The rail's numbers for one type (see `FacetFilter`). */
export interface FacetCounts {
  total: number;
  libraries: Count[];
  categories: Count[];
  sources: Count[];
  publishers: Count[];
  licenses: Count[];
  formats: Count[];
  tags: Count[];
  tagTotal: number;
}

export const facetCounts = (kind: string, visibility: VisibilityRules, tagLimit: number, selectedTags: readonly string[]): Promise<FacetCounts> =>
  invoke("facet_counts", { kind, visibility, tagLimit, selectedTags });

export const searchTags = (kind: string, visibility: VisibilityRules, text: string, limit: number): Promise<Count[]> =>
  invoke("search_tags", { kind, visibility, text, limit });

/** Items per type, each once. */
export const typeCounts = (visibility: VisibilityRules): Promise<Count[]> => invoke("type_counts", { visibility });

export const listCollections = (kind: string, visibility: VisibilityRules): Promise<{ name: string; count: number }[]> =>
  invoke("list_collections", { kind, visibility });

/** Where else each of these items lives, and which copies are another version of it. Items with no other copy are absent. */
export async function elsewhere(ids: readonly string[], visibility: VisibilityRules): Promise<Map<string, { alsoIn: AlsoIn[]; otherVersions: AlsoIn[] }>> {
  const out = new Map<string, { alsoIn: AlsoIn[]; otherVersions: AlsoIn[] }>();
  if (!ids.length) return out;
  const items = ids.map((id) => {
    const parsed = parseQualified(id);
    if (!parsed) throw new Error(`Not a library item id: ${id}`);
    return { library: parsed.library, id: parsed.id };
  });
  const copy = (c: { library: string; id: string }): AlsoIn => ({ library: c.library, id: qualify(c.library, c.id) });
  for (const entry of await invoke<{ library: string; id: string; alsoIn: { library: string; id: string }[]; otherVersions: { library: string; id: string }[] }[]>("elsewhere", { items, visibility })) {
    if (entry.alsoIn.length || entry.otherVersions.length) out.set(qualify(entry.library, entry.id), { alsoIn: entry.alsoIn.map(copy), otherVersions: entry.otherVersions.map(copy) });
  }
  return out;
}

export const listRigs = (library: string): Promise<RigDefinition[]> => invoke("list_rigs", { library });

/** The rigs of a library and the models and animations bound to one (qualified), for matching animations to models. */
export async function rigCatalog(library: string): Promise<Catalog> {
  const raw = await invoke<{ rigs: RigDefinition[]; assets: Asset[] }>("rig_catalog", { library });
  return { schemaVersion: 0, rigs: raw.rigs, assets: raw.assets.map((item) => qualifyAsset(item, library)) };
}

/** Items of a library that carry personal tags from before library tags (at most `limit`), qualified. */
export async function personalTagAssets(library: string, limit: number): Promise<Asset[]> {
  return (await invoke<Asset[]>("personal_tag_assets", { library, limit })).map((item) => qualifyAsset(item, library));
}

/** Up to `limit` ids (qualified) of a library, after `after`, that have no picture yet; `next` is where to continue. */
export async function pendingThumbs(library: string, after: string, limit: number): Promise<{ ids: string[]; next: string | null }> {
  const page = await invoke<{ ids: string[]; next: string | null }>("pending_thumbs", { request: { library, after, limit } });
  return { ids: page.ids.map((id) => qualify(library, id)), next: page.next };
}

/** Mark items (qualified ids, any libraries) as favourites or not. */
export async function setFavorite(ids: readonly string[], favorite: boolean): Promise<void> {
  for (const [library, raw] of groupIds(ids)) await invoke("set_favorite", { library, ids: raw, favorite });
}

/** Put items into a collection, or take them out. */
export async function setCollection(ids: readonly string[], name: string, member: boolean): Promise<void> {
  for (const [library, raw] of groupIds(ids)) await invoke("set_collection", { library, ids: raw, name, member });
}

/** Choose what an item is previewed on (`null` clears the choice). A built-in choice is sent as it is. */
export async function setPreviewChoice(assetId: string, choice: string | null): Promise<void> {
  const parsed = parseQualified(assetId);
  if (!parsed) throw new Error(`Not a library item id: ${assetId}`);
  const raw = choice && choice.startsWith(`${parsed.library}::`) ? choice.slice(parsed.library.length + 2) : choice;
  await invoke("set_preview_choice", { library: parsed.library, assetId: parsed.id, choice: raw });
}

/** Take personal tags off items of a library (they were moved into it as library tags). `removals` have raw ids. */
export const dropPersonalTags = (library: string, removals: { id: string; tags: string[] }[]): Promise<void> => invoke("drop_personal_tags", { library, removals });
