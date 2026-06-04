/**
 * Virtualized asset grid. Row-virtualizes the filtered list with
 * @tanstack/react-virtual: the flat list is chunked into rows of N columns,
 * where N is derived from the measured container width. Only visible rows mount,
 * so 1535 cards stay responsive.
 */

import { useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";

import type { Asset } from "../domain/catalog";
import { AssetCard } from "./AssetCard";
import { useElementWidth } from "./useElementWidth";
import "./AssetGrid.css";

const MIN_CARD_PX = 200;
const GAP_PX = 12;
const ROW_HEIGHT_PX = 244; // card aspect + bar, kept in sync with AssetCard.css

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

  const rows = useMemo(() => chunk(assets, columns), [assets, columns]);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT_PX + GAP_PX,
    overscan: 4,
  });

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
