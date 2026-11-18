import { useMemo, type ReactNode } from "react";
import type { MaterialAsset, ModelAsset } from "../domain/catalog";
import { staticPreviewSource } from "../services/staticPreviewSource";
import { LiveScenePreview } from "./LiveScenePreview";
import { usePreviewPreferences } from "../state/previewPreferences";
import type { HoverScenePreview } from "../services/hoverScenePreview";
import type { PreviewBrightness } from "../domain/previewBrightness";

export function StaticTilePreview({asset,active,fallback,pool,brightness}:{asset:ModelAsset|MaterialAsset;active:boolean;fallback:ReactNode;pool?:HoverScenePreview;brightness?:PreviewBrightness}) {
  const style=usePreviewPreferences(state=>state.materialStyle);
  const shape=asset.kind==="material"?style:"sphere";
  const root=asset.library;
  const content=asset.kind==="model"?asset.fileset:asset.material;
  const source=useMemo(()=>staticPreviewSource(asset,[content,root,shape],shape),[asset.id,content,root,shape]);
  return <LiveScenePreview active={active} source={source} fallback={fallback} draggable pool={pool} brightness={brightness}/>;
}
