/**
 * The library thumbnail of any item as a panel widget (`ctx.widgets.AssetThumbnail`):
 * the picture the grid shows, or what the grid shows while there is none. The
 * item is looked up by its qualified id, in whichever library it lives.
 */

import type { AssetThumbnailProps } from "@ldlework/toybox-sdk";
import type { Asset } from "../../domain/catalog";
import { useAsset } from "../../state/assetCache";
import { useThumbUrl } from "../useThumbUrl";
import "./AssetThumbnailWidget.css";

export function AssetThumbnailWidget({ id, size = 64, alt = "" }: AssetThumbnailProps) {
  const asset = useAsset(id);
  return <span className="asset-thumbnail-widget" style={{ width: size, height: size }}>
    {asset ? <Picture asset={asset} alt={alt} /> : <Note text="No preview" alt={alt} />}
  </span>;
}

function Picture({ asset, alt }: { asset: Asset; alt: string }) {
  const url = useThumbUrl(asset);
  if (url) return <img src={url} alt={alt} />;
  const state = asset.thumb.state;
  return <Note text={state === "rendering" ? "Making preview…" : state === "error" ? "Preview unavailable" : "No preview"} alt={alt} rendering={state === "rendering"} />;
}

/** Stands in for the picture; named like it when the picture has a name. */
function Note({ text, alt, rendering = false }: { text: string; alt: string; rendering?: boolean }) {
  return <span className={`asset-thumbnail-widget__note${rendering ? " is-rendering" : ""}`} role={alt ? "img" : undefined} aria-label={alt || undefined}>{text}</span>;
}
