/**
 * The one favorite affordance, shared by the grid card and the 3D viewer: a
 * borderless star that fills gold when favorited (muted outline → warms toward
 * gold on hover → solid gold when on). Chrome-free so each surface can place it
 * inline; callers own layout, this owns the star and its toggle semantics.
 */

import type { MouseEvent } from "react";
import { Star } from "lucide-react";

import "./FavoriteStar.css";

interface Props {
  favorited: boolean;
  onToggle: () => void;
  /** Forwarded so the card (click-to-select) can stop the toggle bubbling. */
  onClick?: (e: MouseEvent) => void;
}

export function FavoriteStar({ favorited, onToggle, onClick }: Props) {
  const label = favorited ? "Favorited" : "Favorite";
  return (
    <button
      className={`fav-star ${favorited ? "is-on" : ""}`}
      onClick={(e) => {
        onClick?.(e);
        onToggle();
      }}
      title={label}
      aria-label={label}
    >
      <Star size={16} strokeWidth={1.75} fill={favorited ? "currentColor" : "none"} />
    </button>
  );
}
