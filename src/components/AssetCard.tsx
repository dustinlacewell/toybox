/**
 * One asset in the grid: cached thumbnail (or a state-aware placeholder), name,
 * selection checkbox, and favorite toggle. Image-only — no per-card 3D canvas.
 * Clicking the thumbnail opens the asset in the viewer.
 */

import { memo, type MouseEvent } from "react";
import { Film } from "lucide-react";

import type { Asset } from "../domain/catalog";
import { FavoriteStar } from "./FavoriteStar";
import { useThumbUrl } from "./useThumbUrl";
import "./AssetCard.css";

interface Props {
  asset: Asset;
  selected: boolean;
  onToggleSelect: (id: string) => void;
  onToggleFavorite: (id: string) => void;
  onOpen: (id: string) => void;
}

function AssetCardImpl({
  asset,
  selected,
  onToggleSelect,
  onToggleFavorite,
  onOpen,
}: Props) {
  const thumb = useThumbUrl(asset);

  // Shift-click anywhere on the card toggles selection, pre-empting the
  // thumbnail/favorite buttons via capture phase.
  const handleShiftToggle = (e: MouseEvent) => {
    if (!e.shiftKey) return;
    e.preventDefault();
    e.stopPropagation();
    onToggleSelect(asset.id);
  };

  // Plain click on the card body selects. Shift is already handled in the
  // capture phase above, so skip it here to avoid a double toggle. Children
  // that own a click (thumbnail, favorite, checkbox) stopPropagation to opt out.
  const handlePlainSelect = (e: MouseEvent) => {
    if (e.shiftKey) return;
    onToggleSelect(asset.id);
  };

  return (
    <div
      className={`card ${selected ? "card--selected" : ""}`}
      onClickCapture={handleShiftToggle}
      onClick={handlePlainSelect}
    >
      <button
        className="card__thumb"
        onClick={(e) => {
          e.stopPropagation();
          onOpen(asset.id);
        }}
        title={asset.name}
      >
        {thumb ? (
          <img src={thumb} alt={asset.name} loading="lazy" />
        ) : (
          <ThumbPlaceholder state={asset.thumb.state} />
        )}
        {asset.animation.clipCount > 0 && (
          <span
            className="card__anim"
            title={`${asset.animation.clipCount} animation clip${asset.animation.clipCount === 1 ? "" : "s"}`}
          >
            <Film size={13} strokeWidth={2.5} />
          </span>
        )}
      </button>

      <div className="card__bar">
        <input
          type="checkbox"
          checked={selected}
          onChange={() => onToggleSelect(asset.id)}
          onClick={(e) => e.stopPropagation()}
          title="Select for export"
        />
        <span className="card__name" title={asset.name}>
          {asset.name}
        </span>
        <FavoriteStar
          favorited={asset.user.favorite}
          onToggle={() => onToggleFavorite(asset.id)}
          // stopPropagation: the card body is click-to-select; the favorite
          // toggle must not bubble into selection.
          onClick={(e) => e.stopPropagation()}
        />
      </div>
    </div>
  );
}

function ThumbPlaceholder({ state }: { state: Asset["thumb"]["state"] }) {
  const label =
    state === "rendering" ? "rendering…" : state === "error" ? "render failed" : "no preview";
  return <div className={`card__placeholder card__placeholder--${state}`}>{label}</div>;
}

export const AssetCard = memo(AssetCardImpl);
