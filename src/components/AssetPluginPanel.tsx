/**
 * The viewer panel of the plugin an asset came from. A plugin that generates
 * assets — a material rendered from a procedural source, say — ships an
 * `assetPanel`; the viewer shows it as a tab for every item that plugin
 * imported, so the item can be regenerated without leaving it.
 */

import { useEffect, useMemo, useState, type ReactNode } from "react";

import type { AssetPanelCtx, SeedEntryInput } from "@ldlework/toybox-sdk";

import type { Asset } from "../domain/catalog";
import { qualify, unqualifyEntry } from "../domain/libraryId";
import { unwritableReason } from "../domain/transfer";
import { withOrigin } from "../domain/origin";
import { buildPluginBase } from "../services/pluginCtx";
import { ensureAssetFiles, invalidateThumb, mergeSeedEntries, type PluginManifestDto } from "../services/tauriApi";
import { usePlugins } from "../state/plugins";
import { useStore } from "../state/store";
import { MaterialPreviewWidget } from "./plugin-ui/MaterialPreviewWidget";
import { PluginSlot } from "./plugin-ui/PluginSlot";

/** The enabled plugin that imported `asset` and ships a panel for it. An item
 *  imported by a retired plugin belongs to the plugin that replaced it. */
export function useAssetPanelPlugin(asset: Asset): PluginManifestDto | undefined {
  const origin = asset.origin?.plugin;
  return usePlugins((state) => (origin ? state.enabled.find((plugin) => plugin.ui?.assetPanel && (plugin.id === origin || plugin.replaces?.includes(origin))) : undefined));
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
    widgets: { MaterialPreview: MaterialPreviewWidget },
    commit: async (entries: SeedEntryInput[]) => {
      // The item stays in its own library; a plugin that reuses `asset.id` (qualified) replaces it in place.
      const library = asset.library;
      const raw = entries.map((entry) => unqualifyEntry(library, withOrigin(entry, plugin.id)));
      await mergeSeedEntries(library, raw);
      // An item replaced in place has new files behind its old picture; the thumbnail jobs make the new one.
      const changed = raw.map((entry) => qualify(library, entry.id));
      await Promise.all(changed.map((id) => invalidateThumb(id).catch(() => undefined)));
      useStore.getState().bumpData();
    },
  }), [asset, plugin]);
  if (files === "loading") return <p className="viewer-panel-subtitle" role="status">Downloading this item's files…</p>;
  if (files !== "ready") return <p className="viewer-panel-subtitle" role="alert">{files}</p>;
  const cannotKeep = library && unwritableReason(library);
  return <>
    {cannotKeep && <p className="viewer-panel-subtitle" role="note">Changes cannot be kept here: {library.name} ({cannotKeep}).</p>}
    <PluginSlot pluginId={plugin.id} moduleRel={plugin.ui!.assetPanel!} ctx={ctx} fallback={<p className="viewer-panel-subtitle">This plugin's panel could not be shown.</p>} />
  </>;
}
