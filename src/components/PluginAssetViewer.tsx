/**
 * The full view of a plugin-defined asset. The host owns the frame — title,
 * details, favourite, export — and mounts the type's own `Viewer` on the stage.
 * A type with no viewer (or whose plugin is not installed) still opens, showing
 * its thumbnail and details.
 */

import { useMemo, useState, type ReactNode } from "react";

import type { AssetViewerCtx } from "@ldlework/toybox-sdk";
import type { PluginAsset } from "../domain/catalog";
import { pluginAssetFiles } from "../services/pluginAssetFiles";
import { useAssetType } from "../state/assetTypes";
import { SlotErrorBoundary } from "./plugin-ui/SlotErrorBoundary";
import { useThumbUrl } from "./useThumbUrl";
import { ViewerFrame } from "./ViewerFrame";

interface Props { asset: PluginAsset; onClose: () => void; onExport?: () => void; }

export function PluginAssetViewer({ asset, onClose, onExport }: Props) {
  const type = useAssetType(asset.type);
  const Viewer = type.implementation?.Viewer;
  const [sidebar, setSidebar] = useState<ReactNode>(null);
  const [footer, setFooter] = useState<ReactNode>(null);
  const thumb = useThumbUrl(asset);
  const ctx = useMemo<AssetViewerCtx>(() => ({ asset, files: pluginAssetFiles, setSidebar, setFooter }), [asset]);
  const fallback = <div className="viewer__plugin-fallback">
    {thumb && <img src={thumb} alt="" />}
    <p role="status">{Viewer ? `The ${type.singular.toLowerCase()} viewer stopped working. Its files are unaffected.`
      : type.pluginId ? `${type.label} have no viewer.` : `Install the plugin that provides ${type.label.toLowerCase()} to view this item.`}</p>
  </div>;
  return <ViewerFrame asset={asset} onClose={onClose} onExport={onExport} sidebar={sidebar ?? undefined} sidebarLabel={type.singular} footer={footer ?? undefined}>
    {Viewer && type.pluginId
      ? <SlotErrorBoundary pluginId={type.pluginId} moduleRel={`viewer:${type.id}`} fallback={fallback}><Viewer ctx={ctx} /></SlotErrorBoundary>
      : fallback}
  </ViewerFrame>;
}
