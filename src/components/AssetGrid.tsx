/**
 * Virtualized asset grid over a list that is not held: it is given the length of the list and a reader for the
 * rows that have arrived, and says which rows are on screen so the pages for them are fetched. Rows that have not
 * arrived show a placeholder. Cards are progressive — each column is `1fr`, so a
 * card (and its square thumbnail) grows to fill the available width. The row
 * height therefore is NOT a constant: it's the actual column width for the
 * current container plus the bar. Feeding that derived height to the
 * virtualizer keeps every measured slot equal to what renders, so rows pack
 * flush — one card + one gap apart — and never overlap at any width.
 * Only visible rows mount.
 *
 * A browser limits how tall an element can be (about 33 million pixels), which a list of about 900,000 items
 * with 6 columns needs. Rows are therefore placed by `domain/scaledScroll`: while the list fits the scroll
 * position is the list position; beyond that the scrollbar covers the whole list and a scroll position maps
 * linearly onto a position in the list.
 */

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import type { Asset, AssetKind } from "../domain/catalog";
import { useAssetType } from "../state/assetTypes";
import { AssetCard } from "./AssetCard";
import { useElementWidth } from "./useElementWidth";
import { selectItems } from "../domain/selection";
import { geometry, rowPosition, scrollTopForTopRow, topRow, windowRows } from "../domain/scaledScroll";
import { getAudioAudition, stopAudioAudition } from "../services/audioAudition";
import { usePreviewPreferences } from "../state/previewPreferences";
import "./AssetGrid.css";

const BAR_PX = 46; // Controlled tile caption (38px) plus its 8px gap.
const GAP_PX = 18;
/** The rows on screen are reported after the scrolling has paused this long. */
const REPORT_MS = 40;

interface Props {
  inspectedId?: string;
  onInspect?: (id: string) => void;
  /** The type shown (its tile shape). */
  kind: AssetKind;
  /** Items in the list. */
  total: number;
  /** The item at a row, once its page has arrived. */
  rowAt: (index: number) => Asset | undefined;
  /** Changes whenever rows arrive or change, so the grid re-reads them. */
  version: number;
  /** The rows `first..last` are on screen. */
  onRange: (first: number, last: number) => void;
  /** The ids of the rows `from..to` (inclusive), for a Shift range. */
  rangeIds: (from: number, to: number) => Promise<string[]>;
  /** Changes when the list is a different one (another filter): the audition stops. */
  listKey: string;
  selection: Set<string>;
  onSelectionChange: (ids: string[]) => void;
  onToggleFavorite: (id: string) => void;
  onOpen: (id: string) => void;
  /** Name each tile's type (the list shows several). */
  showType?: boolean;
}

export function AssetGrid({
  kind,
  total,
  rowAt,
  version,
  onRange,
  rangeIds,
  listKey,
  selection,
  onSelectionChange,
  onToggleFavorite,
  onOpen,
  onInspect,
  inspectedId,
  showType = false,
}: Props) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const cardSize=usePreviewPreferences(state=>state.cardSize);
  const setCardSize=usePreviewPreferences(state=>state.setCardSize);
  const resizeAnchor=useRef<{index:number;fraction:number}|null>(null);
  useEffect(()=>()=>stopAudioAudition(),[]);
  useEffect(()=>{if(getAudioAudition().assetId)stopAudioAudition();},[listKey]);
  const anchor = useRef<{ id: string; index: number } | null>(null);
  useEffect(() => { if (!selection.size) anchor.current = null; }, [selection]);
  const selectionRef = useRef(selection); selectionRef.current = selection;
  const select = (asset: Asset, index: number, mode: "replace" | "toggle" | "range") => {
    const held = anchor.current;
    if (mode === "range" && held) {
      // The rows between the anchor and the target may not be held: their ids are fetched.
      const from = Math.min(held.index, index), to = Math.max(held.index, index);
      void rangeIds(from, to).then(order => {
        const next = selectItems(order, selectionRef.current, held.id, asset.id, "range");
        onSelectionChange(next.ids);
      });
      return;
    }
    const next = selectItems([asset.id], selectionRef.current, held?.id ?? null, asset.id, mode === "range" ? "toggle" : mode);
    anchor.current = { id: next.anchor ?? asset.id, index };
    onSelectionChange(next.ids);
  };
  const width = useElementWidth(scrollRef);

  const columns = Math.max(1, Math.floor((width + GAP_PX) / (cardSize + GAP_PX)));
  const rowCount = Math.ceil(total / columns);

  // Width one `1fr` card actually occupies — container minus the inter-column
  // gaps, split evenly — and thus the square thumbnail's height. The row height
  // is this plus the bar, so the virtualizer's slot matches the rendered card
  // exactly. Before first measure, fall back to the minimum.
  const colWidth = width > 0 ? (width - GAP_PX * (columns - 1)) / columns : cardSize;
  const previewAspectRatio=useAssetType(kind).tileAspectRatio;
  const rowHeight = colWidth/previewAspectRatio + BAR_PX;

  // The scroll element's height and position. Rows are windowed here (not by a virtualizer) so the placement can
  // be scaled when the list is taller than an element can be.
  const [viewport, setViewport] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);
  useEffect(() => {
    const element = scrollRef.current; if (!element) return;
    const measure = () => setViewport(element.clientHeight);
    const observer = new ResizeObserver(measure); observer.observe(element); measure();
    let frame = 0;
    const scrolled = () => { if (!frame) frame = requestAnimationFrame(() => { frame = 0; setScrollTop(element.scrollTop); }); };
    element.addEventListener("scroll", scrolled, { passive: true });
    return () => { observer.disconnect(); element.removeEventListener("scroll", scrolled); if (frame) cancelAnimationFrame(frame); };
  }, []);
  const stride = rowHeight + GAP_PX;
  const geo = useMemo(() => geometry(rowCount, stride, viewport), [rowCount, stride, viewport]);
  const span = windowRows(geo, scrollTop, 4);
  const rows = Array.from({ length: Math.max(0, span.last - span.first + 1) }, (_, offset) => span.first + offset);

  // Tell the list which rows are on screen, a moment after the scrolling pauses (a jump along the scrollbar
  // passes thousands of rows nobody sees).
  const firstRow = rows.length ? rows[0] * columns : 0;
  const lastRow = rows.length ? Math.min((rows[rows.length - 1] + 1) * columns - 1, total - 1) : 0;
  const reported = useRef(false);
  useEffect(() => {
    if (!total) return;
    const report = () => { reported.current = true; onRange(firstRow, lastRow); };
    if (!reported.current) { report(); return; }
    const timer = window.setTimeout(report, REPORT_MS);
    return () => window.clearTimeout(timer);
  }, [firstRow, lastRow, total, onRange]);
  useEffect(() => { reported.current = false; }, [listKey]);

  const geoRef = useRef(geo); geoRef.current = geo;
  useEffect(()=>{
    const element=scrollRef.current;if(!element)return;
    const wheel=(event:WheelEvent)=>{
      if(!event.ctrlKey)return;
      event.preventDefault();
      const delta=event.deltaY*(event.deltaMode===1?16:event.deltaMode===2?element.clientHeight:1);
      if(!delta)return;
      const row=topRow(geoRef.current,element.scrollTop);
      resizeAnchor.current={index:Math.floor(row)*columns,fraction:row%1};
      setCardSize(Math.round(cardSize*Math.exp(-Math.max(-200,Math.min(200,delta))*.0015)));
    };
    element.addEventListener("wheel",wheel,{passive:false});
    return()=>element.removeEventListener("wheel",wheel);
  },[cardSize,columns,rowHeight,setCardSize]);

  // After a zoom the row that was at the top stays there.
  useLayoutEffect(()=>{
    const anchor=resizeAnchor.current;if(!anchor)return;
    resizeAnchor.current=null;
    const element=scrollRef.current;if(!element)return;
    element.scrollTop=scrollTopForTopRow(geo,Math.floor(anchor.index/columns)+anchor.fraction);
    setScrollTop(element.scrollTop);
  },[cardSize,columns,rowHeight,geo]);

  // A different list starts at the top.
  useEffect(() => { scrollRef.current?.scrollTo({ top: 0 }); setScrollTop(0); }, [listKey]);

  // `version` is read so a page that arrives repaints its rows.
  void version;
  return (
    <div ref={scrollRef} className="grid-scroll">
      <div className="grid-sizer" style={{ height: geo.physicalHeight }}>
        {rows.map((row) => (
          <div
            key={row}
            className="grid-row"
            style={{
              transform: `translateY(${rowPosition(geo, row, scrollTop)}px)`,
              gridTemplateColumns: `repeat(${columns}, 1fr)`,
            }}
          >
            {Array.from({ length: Math.min(columns, total - row * columns) }, (_, column) => {
              const index = row * columns + column;
              const asset = rowAt(index);
              if (!asset) return <div key={`placeholder-${index}`} className="grid-placeholder" style={{ height: rowHeight }} aria-hidden="true" />;
              return (
                <AssetCard
                  key={asset.id}
                  asset={asset}
                  showType={showType}
                  previewAspectRatio={previewAspectRatio}
                  selected={selection.has(asset.id)}
                  active={asset.id === inspectedId}
                  onToggleSelect={(_id, shiftKey) => select(asset, index, shiftKey ? "range" : "toggle")}
                  onToggleFavorite={onToggleFavorite}
                  onOpen={(id)=>{stopAudioAudition();onOpen(id);}}
                  onAudition={onInspect}
                  onInspect={() => { stopAudioAudition();select(asset, index, "replace"); onInspect?.(asset.id); }}
                />
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}
