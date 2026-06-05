/**
 * Virtualized asset grid. Cards are progressive — each column is `1fr`, so a
 * card (and its square thumbnail) grows to fill the available width. The row
 * height therefore is NOT a constant: it's the actual column width for the
 * current container plus the bar. Feeding that derived height to the
 * virtualizer keeps every measured slot equal to what renders, so rows pack
 * flush — one card + one gap apart — and never overlap at any width.
 * Only visible rows mount, keeping 1535 cards responsive.
 */

import { useEffect, useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";

import type { Asset } from "../domain/catalog";
import { AssetCard } from "./AssetCard";
import { useElementWidth } from "./useElementWidth";
import "./AssetGrid.css";

const MIN_CARD_PX = 200; // smallest a card may shrink before dropping a column
const BAR_PX = 37; // .card__bar fixed height — see AssetCard.css
const GAP_PX = 12;

interface Props {
  assets: Asset[];
  selection: Set<string>;
  onToggleSelect: (id: string) => void;
  onToggleFavorite: (id: string) => void;
  onOpen: (id: string) => void;
}

export function AssetGrid({
  assets,
  selection,
  onToggleSelect,
  onToggleFavorite,
  onOpen,
}: Props) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const width = useElementWidth(scrollRef);

  const columns = Math.max(1, Math.floor((width + GAP_PX) / (MIN_CARD_PX + GAP_PX)));
  const rowCount = Math.ceil(assets.length / columns);

  // Width one `1fr` card actually occupies — container minus the inter-column
  // gaps, split evenly — and thus the square thumbnail's height. The row height
  // is this plus the bar, so the virtualizer's slot matches the rendered card
  // exactly. Before first measure, fall back to the minimum.
  const colWidth = width > 0 ? (width - GAP_PX * (columns - 1)) / columns : MIN_CARD_PX;
  const rowHeight = colWidth + BAR_PX;

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
                  selected={selection.has(asset.id)}
                  onToggleSelect={onToggleSelect}
                  onToggleFavorite={onToggleFavorite}
                  onOpen={onOpen}
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
