/**
 * Scrolling a list taller than a browser can make an element (about 33 million pixels; this stays well below it).
 *
 * The list has a *virtual* height (`rows * stride`) and the scroll element a *physical* one, never more than
 * `MAX_SCROLL_PX`. When the list fits, the two are the same and nothing here changes anything. When it does not,
 * the scrollbar covers the whole list and a physical scroll position maps linearly onto a virtual one, from
 * the top of row 0 to the bottom of the last row. Rows are placed at `scrollTop + (rowTop - virtualTop)`, i.e.
 * where they are on screen, so what is on screen looks the same at any scale. Pure functions, no runtime imports
 * (tests load the file on its own).
 */

/** The tallest scroll element made. Chromium allows about 33.5 million pixels, Firefox about 17.9 million. */
export const MAX_SCROLL_PX = 12_000_000;

export interface ScrollGeometry {
  /** Height of a row plus the gap below it. */
  stride: number;
  rowCount: number;
  /** Visible height of the scroll element. */
  viewport: number;
  /** Height the list would have if an element could be that tall. */
  virtualHeight: number;
  /** Height of the scroll element's content. */
  physicalHeight: number;
  /** Virtual pixels per physical pixel of scrolling travel: 1 while the list fits. */
  scale: number;
}

export function geometry(rowCount: number, stride: number, viewport: number, maxPx = MAX_SCROLL_PX): ScrollGeometry {
  const rows = Math.max(0, Math.floor(rowCount));
  const safeStride = Math.max(1, stride);
  const virtualHeight = rows * safeStride;
  const physicalHeight = Math.min(virtualHeight, Math.max(maxPx, viewport + 1));
  const travelPhysical = physicalHeight - viewport;
  const travelVirtual = virtualHeight - viewport;
  const scale = physicalHeight >= virtualHeight || travelPhysical <= 0 || travelVirtual <= 0 ? 1 : travelVirtual / travelPhysical;
  return { stride: safeStride, rowCount: rows, viewport, virtualHeight, physicalHeight, scale };
}

/** The farthest the element scrolls. */
export function maxScrollTop(g: ScrollGeometry): number {
  return Math.max(0, g.physicalHeight - g.viewport);
}

/** The position in the list (virtual pixels from the top of row 0) that the top of the viewport is at. */
export function virtualTop(g: ScrollGeometry, scrollTop: number): number {
  const top = Math.min(Math.max(scrollTop, 0), maxScrollTop(g));
  if (g.scale === 1) return top;
  // The end maps exactly to the end, whatever the rounding of the scale.
  if (top >= maxScrollTop(g)) return Math.max(0, g.virtualHeight - g.viewport);
  return top * g.scale;
}

/** The scroll position whose viewport top is at `virtual` (the inverse of [`virtualTop`]). */
export function scrollTopFor(g: ScrollGeometry, virtual: number): number {
  const wanted = Math.min(Math.max(virtual, 0), Math.max(0, g.virtualHeight - g.viewport));
  if (g.scale === 1) return wanted;
  return Math.min(maxScrollTop(g), Math.round(wanted / g.scale));
}

/** The rows to mount for a scroll position: `first..last` (inclusive), `overscan` rows beyond the viewport. */
export function windowRows(g: ScrollGeometry, scrollTop: number, overscan = 4): { first: number; last: number } {
  if (g.rowCount <= 0) return { first: 0, last: -1 };
  const top = virtualTop(g, scrollTop);
  const first = Math.floor(top / g.stride) - overscan;
  const last = Math.ceil((top + g.viewport) / g.stride) - 1 + overscan;
  return { first: Math.max(0, first), last: Math.min(g.rowCount - 1, Math.max(last, 0)) };
}

/** Where row `row` is placed inside the scroll element's content (pixels from its top). */
export function rowPosition(g: ScrollGeometry, row: number, scrollTop: number): number {
  const top = Math.min(Math.max(scrollTop, 0), maxScrollTop(g));
  return top + (row * g.stride - virtualTop(g, top));
}

/** The scroll position that brings row `row` into view: at the top (`start`), the bottom (`end`), the middle, or the nearest edge (`auto`, which keeps `current` when it is already on screen). */
export function scrollTopForRow(g: ScrollGeometry, row: number, align: "start" | "end" | "center" | "auto" = "auto", current = 0): number {
  const r = Math.min(Math.max(row, 0), Math.max(g.rowCount - 1, 0));
  const rowTop = r * g.stride;
  const rowBottom = rowTop + g.stride;
  const top = virtualTop(g, current);
  if (align === "auto") {
    if (rowTop >= top && rowBottom <= top + g.viewport) return Math.min(Math.max(current, 0), maxScrollTop(g));
    return scrollTopFor(g, rowTop < top ? rowTop : rowBottom - g.viewport);
  }
  if (align === "start") return scrollTopFor(g, rowTop);
  if (align === "end") return scrollTopFor(g, rowBottom - g.viewport);
  return scrollTopFor(g, rowTop - (g.viewport - g.stride) / 2);
}

/** The row (a fractional row: the part is how far into it the top of the viewport is) at the top of the viewport. */
export function topRow(g: ScrollGeometry, scrollTop: number): number {
  return virtualTop(g, scrollTop) / g.stride;
}

/** The scroll position that puts fractional row `row` at the top of the viewport (keeps the place when the layout changes). */
export function scrollTopForTopRow(g: ScrollGeometry, row: number): number {
  return scrollTopFor(g, row * g.stride);
}
