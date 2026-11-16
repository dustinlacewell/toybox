import { useMemo, useSyncExternalStore, type ReactNode } from "react";
import type { AnimationAsset } from "../domain/catalog";
import { animationPreviewSource } from "../services/animationPreviewSource";
import { getRigProfiles, subscribeRigAdapters } from "../services/rigAdapters";
import { useStore } from "../state/store";
import { LiveScenePreview } from "./LiveScenePreview";
import type { HoverScenePreview } from "../services/hoverScenePreview";
import type { PreviewBrightness } from "../domain/previewBrightness";

export function AnimationTilePreview({asset,active,fallback,pool,brightness}:{asset:AnimationAsset;active:boolean;fallback:ReactNode;pool?:HoverScenePreview;brightness?:PreviewBrightness}) {
  const profiles=useSyncExternalStore(subscribeRigAdapters,getRigProfiles);
  const rigs=useStore(state=>state.catalog?.rigs),root=useStore(state=>state.catalog?.libraryRoot);
  const source=useMemo(()=>animationPreviewSource(asset,[asset.fileset,asset.user.previewAssetId,profiles,rigs,root]),[asset.id,asset.fileset,asset.user.previewAssetId,profiles,rigs,root]);
  return <LiveScenePreview active={active} source={source} fallback={fallback} draggable pool={pool} brightness={brightness}/>;
}
