import { Button, Pane } from "@ldlework/toybox-sdk/ui";
import { variantInfo } from "@ldlework/toybox-sdk";
import type { Asset } from "../domain/catalog";
import { useThumbUrl } from "./useThumbUrl";
import { FavoriteStar } from "./FavoriteStar";
import { AssetProperties } from "./AssetProperties";
import { AudioPreview } from "./AudioPreview";
import { StaticTilePreview } from "./StaticTilePreview";
import { AnimationTilePreview } from "./AnimationTilePreview";
import { inspectorScenePreview } from "../services/hoverScenePreview";
import { sourceLabel } from "../domain/facets";
import { useState } from "react";
import type { PreviewBrightness } from "../domain/previewBrightness";
import { ViewportBrightnessControl } from "./viewer/ViewportBrightnessControl";

interface Props { asset: Asset; active?: boolean; onOpen: () => void; onFavorite: () => void; onExport: () => void; onCollect: () => void; }
export function AssetInspector({ asset, active=true, onOpen, onFavorite, onExport, onCollect }: Props) {
  const [brightness,setBrightness]=useState<PreviewBrightness>("dim");
  const thumbnail = useThumbUrl(asset);
  const fallback=thumbnail?<img src={thumbnail} alt=""/>:<span>No preview</span>;
  return <Pane className="asset-inspector" footer={<><Button onClick={onCollect}>Add to collection</Button><Button variant="primary" onClick={onExport}>Export</Button></>}>
    {asset.kind === "audio" ? <AudioPreview asset={asset} onOpen={onOpen}/> : <div className="asset-inspector__preview" title="Drag to rotate · Right-drag to zoom">
      {asset.kind==="animation"?<AnimationTilePreview key={asset.id} asset={asset} active={active} fallback={fallback} pool={inspectorScenePreview} brightness={brightness}/>:<StaticTilePreview key={asset.id} asset={asset} active={active} fallback={fallback} pool={inspectorScenePreview} brightness={brightness}/>}
      <div className="asset-inspector__lighting"><ViewportBrightnessControl value={brightness} onChange={setBrightness}/></div>
      <button className="asset-inspector__expand" onClick={onOpen} aria-label="Open preview">↗</button>
    </div>}
    <div className="asset-inspector__title"><div><h2>{variantInfo(asset.name, asset.kind).base}</h2><p>{sourceLabel(asset)}</p></div><FavoriteStar favorited={asset.user.favorite} onToggle={onFavorite} /></div>
    <AssetProperties asset={asset} onOpen={onOpen} onCollect={onCollect}/>
  </Pane>;
}
