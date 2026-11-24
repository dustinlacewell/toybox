/**
 * The full view of a plugin-defined asset. The host owns the frame — title,
 * details, favourite, export — and mounts the type's own `Viewer` on the stage,
 * with the declaring plugin's host, fs, services and widgets, and a `commit`
 * into the item's library. A type with no viewer (or whose plugin is not
 * installed) still opens, showing its thumbnail and details.
 */

import { useEffect, useMemo, useState, type ReactNode } from "react";

import type { AssetViewerCtx } from "@ldlework/toybox-sdk";
import { Spinner } from "@ldlework/toybox-sdk/ui";
import type { PluginAsset } from "../domain/catalog";
import { unwritableReason } from "../domain/transfer";
import { pluginAssetFiles } from "../services/pluginAssetFiles";
import { commitToLibrary } from "../services/pluginCommit";
import { buildPluginBase } from "../services/pluginCtx";
import { ensureAssetFiles } from "../services/tauriApi";
import { useAssetType } from "../state/assetTypes";
import { usePlugins } from "../state/plugins";
import { useStore } from "../state/store";
import { PANEL_WIDGETS } from "./plugin-ui/panelWidgets";
import { SlotErrorBoundary } from "./plugin-ui/SlotErrorBoundary";
import { useThumbUrl } from "./useThumbUrl";
import { ViewerFrame } from "./ViewerFrame";

interface Props { asset: PluginAsset; onClose: () => void; onExport?: () => void; }

const reason = (failure: unknown) => String(failure).replace(/^Error: /, "");

export function PluginAssetViewer({ asset, onClose, onExport }: Props) {
  const type = useAssetType(asset.type);
  const Viewer = type.implementation?.Viewer;
  const plugin = usePlugins((state) => state.enabled.find((item) => item.id === type.pluginId));
  const library = useStore((state) => state.libraries.find((item) => item.id === asset.library));
  const dataVersion = useStore((state) => state.dataVersion);
  const remote = library?.kind === "remote";
  const [sidebar, setSidebar] = useState<ReactNode>(null);
  const [footer, setFooter] = useState<ReactNode>(null);
  const thumb = useThumbUrl(asset);
  // A type that reads its files from disk has them before its viewer mounts. On a remote library that is a
  // download, perhaps of hundreds of megabytes.
  const needsFiles = !!Viewer && !!type.implementation?.needsFiles && remote;
  const [files, setFiles] = useState<"ready" | "loading" | string>(needsFiles ? "loading" : "ready");
  useEffect(() => {
    if (!needsFiles) { setFiles("ready"); return; }
    let active = true;
    setFiles("loading");
    ensureAssetFiles([asset.id]).then(() => { if (active) setFiles("ready"); }, (failure) => { if (active) setFiles(`Could not download this item's files: ${reason(failure)}`); });
    return () => { active = false; };
  }, [asset.id, needsFiles]);
  const ctx = useMemo<AssetViewerCtx>(() => ({
    asset,
    files: pluginAssetFiles,
    setSidebar,
    setFooter,
    ...buildPluginBase(plugin?.id ?? "", plugin?.permissions ?? {}, asset.library),
    widgets: PANEL_WIDGETS,
    commit: (entries) => {
      if (!plugin) return Promise.reject(new Error(`The plugin that provides ${type.label.toLowerCase()} is not on`));
      return commitToLibrary(asset.library, plugin.id, entries);
    },
    ensureFiles: () => (remote ? ensureAssetFiles([asset.id]) : Promise.resolve()),
    dataVersion,
  }), [asset, plugin, remote, dataVersion, type.label]);
  const cannotKeep = library && unwritableReason(library);
  const fallback = <div className="viewer__plugin-fallback">
    {thumb && <img src={thumb} alt="" />}
    <p role="status">{Viewer ? `The ${type.singular.toLowerCase()} viewer stopped working. Its files are unaffected.`
      : type.pluginId ? `${type.label} have no viewer.` : `Install the plugin that provides ${type.label.toLowerCase()} to view this item.`}</p>
  </div>;
  const stage = !Viewer || !type.pluginId ? fallback
    : files === "loading" ? <div className="viewer__plugin-fallback">{thumb && <img src={thumb} alt="" />}<Spinner /><p role="status">Downloading this item's files…</p></div>
    : files !== "ready" ? <div className="viewer__plugin-fallback"><p role="alert">{files}</p></div>
    : <SlotErrorBoundary pluginId={type.pluginId} moduleRel={`viewer:${type.id}`} fallback={fallback}><Viewer ctx={ctx} /></SlotErrorBoundary>;
  const side = sidebar && <>
    {cannotKeep && <p className="viewer-panel-subtitle" role="note">Changes cannot be kept here: {library.name} ({cannotKeep}).</p>}
    {sidebar}
  </>;
  return <ViewerFrame asset={asset} onClose={onClose} onExport={onExport} sidebar={side || undefined} sidebarLabel={type.singular} footer={footer ?? undefined}>
    {stage}
  </ViewerFrame>;
}
