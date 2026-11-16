/**
 * Plugin discovery + run dispatch. On boot the registry lists installed plugins
 * (Rust), loads each entry module (isolated per-plugin so one failure never
 * breaks the rest), and partitions them into exporters / importers / load
 * errors. The export drawer reads `exporters` to render its mode list; a future
 * import panel will read `importers`.
 */

import type { ExporterPlugin, PluginConfig } from "@ldlework/toybox-sdk";
import type { ExportReport } from "./tauriApi";
import { buildExportCtx, selectedAssets } from "./pluginHost";
import type { AssetKind } from "../domain/catalog";
import { loadPlugin } from "./pluginLoader";
import { listPlugins, type PluginManifestDto } from "./tauriApi";
import { nativeExporters } from "./nativeExporters";
import { registerRigAdapters } from "./rigAdapters";

export interface LoadedExporter {
  manifest: PluginManifestDto;
  plugin: ExporterPlugin;
}
/** Importers are panel-only: registered from the manifest, their `importPanel`
 *  slot component does the work. No entry module is loaded. */
export interface LoadedImporter {
  manifest: PluginManifestDto;
}
export interface PluginLoadError {
  manifest: PluginManifestDto;
  error: string;
}

export interface PluginRegistry {
  exporters: LoadedExporter[];
  importers: LoadedImporter[];
  errors: PluginLoadError[];
}

/** Discover + load all installed plugins. Each load is isolated: a throwing
 *  plugin becomes an `errors` entry, never aborting discovery. */
export async function loadRegistry(): Promise<PluginRegistry> {
  const manifests = await listPlugins();
  console.info(`[plugins] discovered ${manifests.length}:`, manifests.map((m) => m.id));
  const registry: PluginRegistry = { exporters: [...nativeExporters], importers: [], errors: [] };

  await Promise.all(
    manifests.map(async (manifest) => {
      // Importers are panel-only — registered from the manifest, no entry module
      // to load (their `importPanel` slot does the work).
      if (manifest.kind === "importer") {
        registry.importers.push({ manifest });
        console.info(`[plugins] registered ${manifest.id} (importer)`);
        return;
      }
      // Exporters load their entry module (it default-exports the `run`).
      try {
        const { plugin } = await loadPlugin(manifest);
        if (manifest.kind === "rigAdapter") {
          registerRigAdapters(plugin as import("@ldlework/toybox-sdk").RigAdapterPlugin);
          return;
        }
        registry.exporters.push({ manifest, plugin: plugin as ExporterPlugin });
        console.info(`[plugins] loaded ${manifest.id} (exporter)`);
      } catch (e) {
        console.error(`[plugins] FAILED to load ${manifest.id}:`, e);
        registry.errors.push({ manifest, error: String(e) });
      }
    }),
  );

  return registry;
}

/** The importers that produce `kind`. A manifest without `assetKinds` is a model
 *  importer. */
export function importersFor(importers: LoadedImporter[], kind: AssetKind): LoadedImporter[] {
  return importers.filter((i) => (i.manifest.assetKinds ?? ["model"]).includes(kind));
}

/** Run one exporter plugin over the current selection and return its report. */
export async function runExporter(
  loaded: LoadedExporter,
  selectedIds: string[],
  config: PluginConfig,
  signal: AbortSignal,
): Promise<ExportReport> {
  const ctx = buildExportCtx(loaded.manifest, config, signal);
  const assets = selectedAssets(selectedIds);
  const kinds = loaded.manifest.assetKinds ?? ["model"];
  if (assets.length !== selectedIds.length || assets.some((a) => !kinds.includes(a.kind))) {
    throw new Error("This exporter does not accept the selected asset type.");
  }
  return loaded.plugin.run(ctx, assets);
}
