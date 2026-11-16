import { memo, useState } from "react";
import { Film } from "lucide-react";
import { CueButton, PreviewTile, Waveform, formatTime } from "@ldlework/toybox-sdk/ui";
import { variantInfo } from "@ldlework/toybox-sdk";
import type { Asset, AudioAsset } from "../domain/catalog";
import { useAudioPreview } from "./useAudioPreview";
import { FavoriteStar } from "./FavoriteStar";
import { useThumbUrl } from "./useThumbUrl";
import { AnimationTilePreview } from "./AnimationTilePreview";
import { StaticTilePreview } from "./StaticTilePreview";
import { sourceLabel } from "../domain/facets";

interface Props {
  asset: Asset; selected: boolean; active?: boolean;
  previewAspectRatio?: number;
  onInspect?: (id: string) => void; onToggleSelect: (id: string, shiftKey?: boolean) => void;
  onAudition?: (id: string) => void;
  onToggleFavorite: (id: string) => void; onOpen: (id: string) => void;
}
export const AssetCard = memo(function AssetCard({ asset, selected, active = false, onInspect, onToggleSelect, onToggleFavorite, onOpen, onAudition, previewAspectRatio }: Props) {
  const thumb = useThumbUrl(asset);
  const [hovered,setHovered]=useState(false);
  const fallback=thumb?<img src={thumb} alt="" loading="lazy"/>:<span>{asset.thumb.state==="rendering"?"Making preview…":asset.thumb.state==="error"?"Preview unavailable":"No preview"}</span>;
  return <PreviewTile title={variantInfo(asset.name, asset.kind).base} subtitle={asset.kind === "audio" ? `${formatTime(asset.audio.duration)} · ${asset.audio.format}` : asset.kind === "animation" ? `${asset.clip.sourceName} · ${asset.clip.duration.toFixed(1)} s` : sourceLabel(asset)}
    active={active} selected={selected} onPress={() => onInspect ? onInspect(asset.id) : onOpen(asset.id)} onOpen={() => {setHovered(false);onOpen(asset.id);}}
    onPreviewHoverChange={asset.kind!=="audio"?setHovered:undefined}
    onSelectedChange={(_selected, shiftKey) => onToggleSelect(asset.id, shiftKey)}
    previewAspectRatio={previewAspectRatio}
    preview={asset.kind === "audio" ? <div className="asset-card__audio-waveform"><Waveform peaks={asset.audio.peaks}/></div> : asset.kind==="animation"?<AnimationTilePreview asset={asset} active={hovered} fallback={fallback}/>:<StaticTilePreview asset={asset} active={hovered} fallback={fallback}/>}
    badge={asset.kind === "animation" || (asset.kind === "model" && asset.animation.clipCount > 0) ? <Film size={13} /> : undefined}
    previewAction={asset.kind==="audio"&&<TileCue asset={asset} onInspect={onAudition}/>}
    actions={<FavoriteStar favorited={asset.user.favorite} onToggle={() => onToggleFavorite(asset.id)} />} />;
});

function TileCue({asset,onInspect}:{asset:AudioAsset;onInspect?:(id:string)=>void}) {
  const cue=useAudioPreview(asset);
  return <span title={cue.error}><CueButton active={cue.active} onCue={()=>{onInspect?.(asset.id);cue.toggle();}}/></span>;
}
