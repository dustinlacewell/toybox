/**
 * Virtualized asset grid. Cards are progressive — each column is `1fr`, so a
 * card (and its square thumbnail) grows to fill the available width. The row
 * height therefore is NOT a constant: it's the actual column width for the
 * current container plus the bar. Feeding that derived height to the
 * virtualizer keeps every measured slot equal to what renders, so rows pack
 * flush — one card + one gap apart — and never overlap at any width.
 * Only visible rows mount, keeping 1535 cards responsive.
 */

import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";

import type { Asset } from "../domain/catalog";
import { AssetCard } from "./AssetCard";
import { useElementWidth } from "./useElementWidth";
import { selectItems } from "../domain/selection";
import { getAudioAudition, stopAudioAudition } from "../services/audioAudition";
import { usePreviewPreferences } from "../state/previewPreferences";
import "./AssetGrid.css";

const BAR_PX = 46; // Controlled tile caption (38px) plus its 8px gap.
const GAP_PX = 18;

interface Props {
  inspectedId?: string;
  onInspect?: (id: string) => void;
  assets: Asset[];
  selection: Set<string>;
  onSelectionChange: (ids: string[]) => void;
  onToggleFavorite: (id: string) => void;
  onOpen: (id: string) => void;
}

export function AssetGrid({
  assets,
  selection,
  onSelectionChange,
  onToggleFavorite,
  onOpen,
  onInspect,
  inspectedId,
}: Props) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const cardSize=usePreviewPreferences(state=>state.cardSize);
  const setCardSize=usePreviewPreferences(state=>state.setCardSize);
  const resizeAnchor=useRef<{index:number;fraction:number}|null>(null);
  useEffect(()=>()=>stopAudioAudition(),[]);
  useEffect(()=>{const id=getAudioAudition().assetId;if(id&&!assets.some(asset=>asset.id===id))stopAudioAudition();},[assets]);
  const anchor = useRef<string | null>(null);
  useEffect(() => { if (!selection.size) anchor.current = null; }, [selection]);
  const select = (id: string, mode: "replace" | "toggle" | "range") => {
    const next = selectItems(assets.map(asset => asset.id), selection, anchor.current, id, mode);
    anchor.current = next.anchor;
    onSelectionChange(next.ids);
  };
  const width = useElementWidth(scrollRef);

  const columns = Math.max(1, Math.floor((width + GAP_PX) / (cardSize + GAP_PX)));
  const rowCount = Math.ceil(assets.length / columns);

  // Width one `1fr` card actually occupies — container minus the inter-column
  // gaps, split evenly — and thus the square thumbnail's height. The row height
  // is this plus the bar, so the virtualizer's slot matches the rendered card
  // exactly. Before first measure, fall back to the minimum.
  const colWidth = width > 0 ? (width - GAP_PX * (columns - 1)) / columns : cardSize;
  const previewAspectRatio=assets[0]?.kind==="audio"?2:1;
  const rowHeight = colWidth/previewAspectRatio + BAR_PX;

  const rows = useMemo(() => chunk(assets, columns), [assets, columns]);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight + GAP_PX,
    overscan: 4,
  });

  // estimateSize is sampled once and cached; re-measure when the derived row
  // height changes (a resize that alters column width) so slots stay correct.
  useEffect(() => {
    virtualizer.measure();
  }, [virtualizer, rowHeight]);

  useEffect(()=>{
    const element=scrollRef.current;if(!element)return;
    const wheel=(event:WheelEvent)=>{
      if(!event.ctrlKey)return;
      event.preventDefault();
      const delta=event.deltaY*(event.deltaMode===1?16:event.deltaMode===2?element.clientHeight:1);
      if(!delta)return;
      const row=element.scrollTop/(rowHeight+GAP_PX);
      resizeAnchor.current={index:Math.floor(row)*columns,fraction:row%1};
      setCardSize(Math.round(cardSize*Math.exp(-Math.max(-200,Math.min(200,delta))*.0015)));
    };
    element.addEventListener("wheel",wheel,{passive:false});
    return()=>element.removeEventListener("wheel",wheel);
  },[cardSize,columns,rowHeight,setCardSize]);

  useLayoutEffect(()=>{
    const anchor=resizeAnchor.current;if(!anchor)return;
    resizeAnchor.current=null;
    virtualizer.measure();
    virtualizer.scrollToOffset((Math.floor(anchor.index/columns)+anchor.fraction)*(rowHeight+GAP_PX));
  },[cardSize,columns,rowHeight,virtualizer]);

  return (
    <div ref={scrollRef} className="grid-scroll">
      <div className="grid-sizer" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((vrow) => {
          const row = rows[vrow.index] ?? [];
          return (
            <div
              key={vrow.key}
              className="grid-row"
              style={{
                transform: `translateY(${vrow.start}px)`,
                gridTemplateColumns: `repeat(${columns}, 1fr)`,
              }}
            >
              {row.map((asset) => (
                <AssetCard
                  key={asset.id}
                  asset={asset}
                  previewAspectRatio={previewAspectRatio}
                  selected={selection.has(asset.id)}
                  active={asset.id === inspectedId}
                  onToggleSelect={(id, shiftKey) => select(id, shiftKey ? "range" : "toggle")}
                  onToggleFavorite={onToggleFavorite}
                  onOpen={(id)=>{stopAudioAudition();onOpen(id);}}
                  onAudition={onInspect}
                  onInspect={(id) => { stopAudioAudition();select(id, "replace"); onInspect?.(id); }}
                />
              ))}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function chunk<T>(items: T[], size: number): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < items.length; i += size) out.push(items.slice(i, i + size));
  return out;
}
