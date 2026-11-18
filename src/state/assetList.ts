/**
 * The list the grid shows, as pages fetched on demand. One list at a time: the filter (already settled by
 * the caller) says which. The grid reports the rows on screen; pages for them are requested (a couple at
 * once, see `domain/pageCache`) and the answers that arrive after the question changed are dropped.
 * A change of data (`dataVersion`) keeps the rows on screen until their fresh pages arrive.
 */

import { useCallback, useEffect, useMemo, useRef, useSyncExternalStore } from "react";

import type { Asset } from "../domain/catalog";
import { filterSignature, type BrowseFilter } from "../domain/browseQuery";
import { PAGE_SIZE, PageCache } from "../domain/pageCache";
import { countAssets, queryAssets } from "../services/browse";
import { cacheAssets, onAssetsPatched } from "./assetCache";
import { useStore } from "./store";

export interface ListState {
  /** Items in the list; null until the first page has answered. */
  total: number | null;
  /** False while `total` is only what the pages seen so far prove (a broad search: the exact count is still being made). */
  exact: boolean;
  /** Moves whenever rows arrive or change, so a view re-reads them. */
  version: number;
  error: string | null;
}

export class AssetList {
  private pages = new PageCache<Asset>(PAGE_SIZE, 40, 2);
  private filter: BrowseFilter | null = null;
  private signature = "";
  private range = { first: 0, last: 0 };
  private state: ListState = { total: null, exact: true, version: 0, error: null };
  /** The exact length, once `count` answered for the current question. */
  private exactTotal: number | null = null;
  private counting = false;
  private listeners = new Set<() => void>();

  constructor(
    private fetchPage: (filter: BrowseFilter, offset: number, limit: number) => Promise<{ total: number; exact?: boolean; assets: Asset[] }> = queryAssets,
    private count: (filter: BrowseFilter) => Promise<number> = countAssets,
  ) {}

  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  getState = (): ListState => this.state;

  private set(next: Partial<ListState>) {
    this.state = { ...this.state, ...next, version: this.state.version + 1 };
    for (const listener of [...this.listeners]) listener();
  }

  /** Ask a (possibly new) question. The same question again refreshes the rows, keeping them on screen. */
  query(filter: BrowseFilter | null, refresh: boolean): void {
    const signature = filter ? filterSignature(filter) : "";
    const same = signature === this.signature && !!filter;
    this.filter = filter;
    this.signature = signature;
    if (!filter) {
      this.pages.reset(false);
      this.exactTotal = null;
      this.counting = false;
      this.set({ total: null, exact: true, error: null });
      return;
    }
    if (same && !refresh) return;
    this.pages.reset(same);
    if (!same || refresh) { this.exactTotal = null; this.counting = false; }
    this.set({ total: same ? this.pages.total : null, error: null });
    this.pump();
  }

  /** The rows `first..last` are on screen. */
  show(first: number, last: number): void {
    this.range = { first, last };
    this.pump();
  }

  rowAt(index: number): Asset | undefined {
    return this.pages.rowAt(index);
  }

  /** Apply a patch to every held row. */
  patch(change: (asset: Asset) => Asset): void {
    if (this.pages.patch(change)) this.set({});
  }

  /** The row index of an item held in the pages, if it is. */
  indexOf(id: string): number {
    return this.pages.held().find(({ row }) => row.id === id)?.index ?? -1;
  }

  /** Ask for the exact length (once per question); the page that said `exact: false` did not wait for it. */
  private countExact(filter: BrowseFilter, generation: number): void {
    if (this.counting || this.exactTotal !== null) return;
    this.counting = true;
    this.count(filter).then(
      (total) => {
        if (generation !== this.pages.generation) return;
        this.exactTotal = total;
        this.counting = false;
        this.pages.setTotal(generation, total);
        this.set({ total, exact: true });
        this.pump();
      },
      (failure) => {
        if (generation !== this.pages.generation) return;
        this.counting = false;
        this.set({ error: String(failure).replace(/^Error: /, "") });
      },
    );
  }

  private pump(): void {
    const filter = this.filter;
    if (!filter) return;
    const { first, last } = this.range;
    for (const page of this.pages.need(first, Math.max(last, first))) {
      const ticket = this.pages.begin(page);
      this.fetchPage(filter, page * PAGE_SIZE, PAGE_SIZE).then(
        ({ total, exact = true, assets }) => {
          cacheAssets(assets);
          // A lower bound never replaces a length that is known, and never shrinks the list under the scrollbar.
          const known = exact ? total : this.exactTotal ?? Math.max(total, this.pages.total ?? 0);
          if (this.pages.accept(ticket, assets, known)) {
            if (exact) this.exactTotal = total;
            this.set({ total: known, exact: exact || this.exactTotal !== null, error: null });
            if (!exact) this.countExact(filter, ticket.generation);
            this.pump();
          }
        },
        (failure) => {
          this.pages.fail(ticket);
          if (ticket.generation === this.pages.generation) this.set({ error: String(failure).replace(/^Error: /, "") });
        },
      );
    }
  }
}

/** The one list of the library view. */
export const assetList = new AssetList();
onAssetsPatched((change) => assetList.patch(change));

/**
 * The list for `filter` (settled by the caller; `null` shows nothing). Returns what the grid needs: the
 * length, a row reader, and a way to say which rows are on screen.
 */
export function useAssetList(filter: BrowseFilter | null) {
  const dataVersion = useStore((state) => state.dataVersion);
  const signature = useMemo(() => (filter ? filterSignature(filter) : ""), [filter]);
  const lastVersion = useRef(dataVersion);
  const state = useSyncExternalStore(assetList.subscribe, assetList.getState);
  useEffect(() => {
    const refresh = lastVersion.current !== dataVersion;
    lastVersion.current = dataVersion;
    assetList.query(filter, refresh);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature, dataVersion]);
  const show = useCallback((first: number, last: number) => assetList.show(first, last), []);
  const rowAt = useCallback((index: number) => assetList.rowAt(index), [state.version]); // eslint-disable-line react-hooks/exhaustive-deps
  return { total: state.total, exact: state.exact, error: state.error, version: state.version, rowAt, show };
}
