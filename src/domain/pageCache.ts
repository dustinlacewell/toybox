/**
 * The window's view of a list too long to hold: rows are fetched in pages as the grid
 * scrolls to them, a few pages are kept, and an answer to a question that is no longer
 * being asked is thrown away. Pure data, no runtime imports (tests load it on its own).
 *
 * The owner asks `need(first, last)` with the rows on screen and sends a request for each
 * page it returns; it answers with `accept(ticket, rows, total)` or `fail(ticket)`.
 * Pages are asked for only when no more than `maxInflight` requests are on their way,
 * and only for the rows currently on screen (and the page beyond each end), so scrolling
 * fast or jumping to the middle of a million rows never queues what is no longer visible.
 */

export const PAGE_SIZE = 100;

/** What a request carries so its answer can be matched to the question it was for. */
export interface PageTicket {
  generation: number;
  page: number;
}

export class PageCache<T> {
  /** Changes whenever the question changes (or the data underneath does): older answers are dropped. */
  generation = 0;
  /** How many rows the list has, once an answer said. */
  total: number | null = null;
  private pages = new Map<number, T[]>();
  /** Pages whose rows are from before the last change of data: shown until the new ones arrive. */
  private stale = new Set<number>();
  private inflight = new Set<number>();
  private used = new Map<number, number>();
  private clock = 0;

  constructor(
    readonly pageSize = PAGE_SIZE,
    /** Pages kept; the ones on screen are never dropped. */
    readonly maxPages = 40,
    /** Requests allowed on their way at once. */
    readonly maxInflight = 2,
  ) {}

  /**
   * Start a new question. With `keepRows` the rows held stay visible (stale) until their page is
   * answered again — a refresh after a write. Without it, everything is forgotten — another filter.
   * Answers to earlier requests are ignored either way.
   */
  reset(keepRows = false): number {
    this.generation++;
    this.inflight.clear();
    if (keepRows) {
      for (const page of this.pages.keys()) this.stale.add(page);
    } else {
      this.pages.clear();
      this.stale.clear();
      this.total = null;
      this.used.clear();
    }
    return this.generation;
  }

  /** The row at `index`, or undefined while its page has not arrived. */
  rowAt(index: number): T | undefined {
    if (index < 0) return undefined;
    return this.pages.get(Math.floor(index / this.pageSize))?.[index % this.pageSize];
  }

  /** Whether the row at `index` is from before the last refresh. */
  isStale(index: number): boolean {
    return this.stale.has(Math.floor(index / this.pageSize));
  }

  has(page: number): boolean {
    return this.pages.has(page) && !this.stale.has(page);
  }

  loadedPages(): number {
    return this.pages.size;
  }

  inflightPages(): number[] {
    return [...this.inflight];
  }

  /**
   * The pages to ask for now so the rows `first..last` (inclusive) can be shown: those not held (or stale)
   * and not already asked for, nearest to the middle of the screen first, at most as many as free slots.
   */
  need(first: number, last: number): number[] {
    const lastRow = this.total === null ? Math.max(first, 0) : Math.min(last, this.total - 1);
    if (this.total !== null && this.total <= 0) return [];
    const from = Math.floor(Math.max(first, 0) / this.pageSize);
    const to = Math.floor(Math.max(lastRow, first, 0) / this.pageSize);
    const lastPage = this.total === null ? to : Math.max(Math.floor((this.total - 1) / this.pageSize), 0);
    // The rows on screen, then one page on either side so a slow scroll finds the next page there.
    const wanted: number[] = [];
    for (let page = Math.max(from - 1, 0); page <= Math.min(to + 1, lastPage); page++) wanted.push(page);
    this.clock++;
    for (let page = from; page <= to; page++) this.used.set(page, this.clock);
    const middle = (from + to) / 2;
    const visible = (page: number) => page >= from && page <= to;
    const missing = wanted.filter((page) => !this.has(page) && !this.inflight.has(page) && (visible(page) || this.total !== null));
    missing.sort((a, b) => Number(visible(b)) - Number(visible(a)) || Math.abs(a - middle) - Math.abs(b - middle));
    return missing.slice(0, Math.max(0, this.maxInflight - this.inflight.size));
  }

  /** Note that `page` is being asked for. */
  begin(page: number): PageTicket {
    this.inflight.add(page);
    return { generation: this.generation, page };
  }

  /** Take an answer. False (and nothing stored) when the question has changed since the request. */
  accept(ticket: PageTicket, rows: T[], total: number): boolean {
    if (ticket.generation !== this.generation) return false;
    this.inflight.delete(ticket.page);
    this.pages.set(ticket.page, rows);
    this.stale.delete(ticket.page);
    this.total = total;
    this.used.set(ticket.page, ++this.clock);
    this.evict(ticket.page);
    return true;
  }

  /** The exact length arrived (separately from a page): take it if the question is still the same one. */
  setTotal(generation: number, total: number): boolean {
    if (generation !== this.generation) return false;
    this.total = total;
    return true;
  }

  /** A request ended without an answer: its slot is free again. */
  fail(ticket: PageTicket): void {
    if (ticket.generation === this.generation) this.inflight.delete(ticket.page);
  }

  /** Apply `change` to every held row; true if any row changed. */
  patch(change: (row: T) => T): boolean {
    let changed = false;
    for (const [page, rows] of this.pages) {
      let copy: T[] | null = null;
      rows.forEach((row, index) => {
        const next = change(row);
        if (next !== row) {
          copy ??= rows.slice();
          copy[index] = next;
          changed = true;
        }
      });
      if (copy) this.pages.set(page, copy);
    }
    return changed;
  }

  /** The held rows, in list order (for finding one by id). */
  held(): { index: number; row: T }[] {
    const out: { index: number; row: T }[] = [];
    for (const [page, rows] of [...this.pages].sort((a, b) => a[0] - b[0])) rows.forEach((row, offset) => out.push({ index: page * this.pageSize + offset, row }));
    return out;
  }

  private evict(keep: number): void {
    while (this.pages.size > this.maxPages) {
      let oldest: number | null = null;
      for (const page of this.pages.keys()) {
        if (page === keep) continue;
        if (oldest === null || (this.used.get(page) ?? 0) < (this.used.get(oldest) ?? 0)) oldest = page;
      }
      if (oldest === null) return;
      this.pages.delete(oldest);
      this.stale.delete(oldest);
      this.used.delete(oldest);
    }
  }
}

/** Rows `first..last` that a viewport shows, given the row height and the rows per grid row. */
export function visibleRows(scrollTop: number, viewport: number, rowHeight: number, columns: number, total: number): { first: number; last: number } {
  if (total <= 0 || rowHeight <= 0) return { first: 0, last: -1 };
  const firstRow = Math.max(Math.floor(scrollTop / rowHeight), 0);
  const lastRow = Math.ceil((scrollTop + viewport) / rowHeight);
  return { first: Math.min(firstRow * columns, total - 1), last: Math.min((lastRow + 1) * columns - 1, total - 1) };
}
