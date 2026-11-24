/**
 * The viewer panel of the plugin an asset came from. A plugin that generates
 * assets — a material rendered from a procedural source, say — ships an
 * `assetPanel`; the viewer shows it as a tab for every item that plugin
 * imported, so the item can be regenerated without leaving it.
 */

import { useEffect, useMemo, useState, type ReactNode } from "react";

import type { AssetPanelCtx } from "@ldlework/toybox-sdk";

import type { Asset } from "../domain/catalog";
import { unwritableReason } from "../domain/transfer";
import { commitToLibrary } from "../services/pluginCommit";
import { showsAssetPanel } from "../services/pluginCapabilities";
import { buildPluginBase } from "../services/pluginCtx";
import { ensureAssetFiles, type PluginManifestDto } from "../services/tauriApi";
import { usePlugins } from "../state/plugins";
import { useStore } from "../state/store";
import { PANEL_WIDGETS } from "./plugin-ui/panelWidgets";
import { PluginSlot } from "./plugin-ui/PluginSlot";

/** The enabled plugin that imported `asset` and ships a panel for it. An item
 *  imported by a retired plugin belongs to the plugin that replaced it; an item
 *  of a type the plugin declares gets no panel (its Viewer is the plugin's UI). */
export function useAssetPanelPlugin(asset: Asset): PluginManifestDto | undefined {
  return usePlugins((state) => state.enabled.find((plugin) => showsAssetPanel(plugin, asset)));
}

export function AssetPluginPanel({ asset, plugin }: { asset: Asset; plugin: PluginManifestDto }): ReactNode {
  // A panel works on the item's files (the archive a material was rendered from).
  // On a remote library they are downloaded first; keeping a change goes through
  // the same upload-and-commit as an import, so it needs a token that may write.
  const library = useStore((state) => state.libraries.find((item) => item.id === asset.library));
  const remote = library?.kind === "remote";
  const [files, setFiles] = useState<"ready" | "loading" | string>(remote ? "loading" : "ready");
  useEffect(() => {
    if (!remote) { setFiles("ready"); return; }
    let active = true;
    setFiles("loading");
    ensureAssetFiles([asset.id], true).then(() => { if (active) setFiles("ready"); }, (failure) => { if (active) setFiles(`Could not download this item's files: ${String(failure).replace(/^Error: /, "")}`); });
    return () => { active = false; };
  }, [asset.id, remote]);
  const ctx = useMemo<AssetPanelCtx>(() => ({
    asset,
    ...buildPluginBase(plugin.id, plugin.permissions, asset.library),
    widgets: PANEL_WIDGETS,
    // The item stays in its own library.
    commit: (entries) => commitToLibrary(asset.library, plugin.id, entries),
  }), [asset, plugin]);
  if (files === "loading") return <p className="viewer-panel-subtitle" role="status">Downloading this item's files…</p>;
  if (files !== "ready") return <p className="viewer-panel-subtitle" role="alert">{files}</p>;
  const cannotKeep = library && unwritableReason(library);
  return <>
    {cannotKeep && <p className="viewer-panel-subtitle" role="note">Changes cannot be kept here: {library.name} ({cannotKeep}).</p>}
    <PluginSlot pluginId={plugin.id} moduleRel={plugin.ui!.assetPanel!} ctx={ctx} fallback={<p className="viewer-panel-subtitle">This plugin's panel could not be shown.</p>} />
  </>;
}
