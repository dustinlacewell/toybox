/**
 * The chrome every asset kind's detail viewer shares: a backdrop that closes on
 * outside click, the panel, a header (name, `pack · category`, favourite star,
 * "Reveal in Explorer", close) and a stage that renders the kind-specific
 * content passed as children. Uses only fields common to every kind of asset.
 */

import type { ReactNode } from "react";
import { FolderOpen } from "lucide-react";

import type { Asset } from "../domain/catalog";
import { revealAsset } from "../services/reveal";
import { IconButton } from "@ldlework/toybox-sdk/ui";
import { FavoriteStar } from "./FavoriteStar";
import { useFavorites } from "./useFavorites";
import "./ViewerFrame.css";

interface Props {
  asset: Asset;
  onClose: () => void;
  children?: ReactNode;
}

export function ViewerFrame({ asset, onClose, children }: Props) {
  const { toggleFavorite } = useFavorites();

  return (
    <div className="viewer" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="viewer__panel">
        <header className="viewer__header">
          <div>
            <div className="viewer__name">{asset.name}</div>
            <div className="viewer__meta">
              {asset.pack} · {asset.category}
            </div>
          </div>
          <div className="viewer__actions">
            <FavoriteStar
              favorited={asset.user.favorite}
              onToggle={() => toggleFavorite(asset.id)}
            />
            <IconButton
              icon={FolderOpen}
              label="Reveal in Explorer"
              onClick={() => void revealAsset(asset)}
            />
            <button className="viewer__close" onClick={onClose} title="Close">
              ✕
            </button>
          </div>
        </header>

        <div className="viewer__stage">{children}</div>
      </div>
    </div>
  );
}
