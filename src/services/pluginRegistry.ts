/**
 * Plugin discovery + run dispatch. On boot the registry lists installed plugins
 * (Rust), keeps the active ones, and loads each entry module (isolated per
 * plugin so one failure never breaks the rest). What a plugin does follows
 * from what it ships: its manifest makes it an importer or declares types, its
 * module may export, carry rig profiles, implement types and contribute to
 * extension points. The export drawer reads `exporters`; the import screen
 * reads `importers`; every type-aware view reads `types`.
 */

import type { ExtensionLookup, ExporterPlugin, PluginConfig } from "@ldlework/toybox-sdk";
import type { ExportReport } from "./tauriApi";
import { buildExportCtx, loadSelectedAssets } from "./pluginHost";
import { assetType, type AssetKind } from "../domain/catalog";
import { loadPlugin } from "./pluginLoader";
import { listPlugins, type PluginManifestDto } from "./tauriApi";
import { nativeExporters } from "./nativeExporters";
import { BUILTIN_RIG_PLUGINS, registerRigAdapters, resetRigAdapters } from "./rigAdapters";
import { NO_RULES, type VisibilityRules } from "../domain/browseQuery";
import { buildAssetTypes, defaultAssetTypes, pluginVisibility, type AssetTypeEntry, type DeclaredTypes } from "./assetTypes";
import { pluginKinds } from "./pluginKinds";
import { isActive, isBlocked } from "./pluginActive";
import { blockedSentence } from "./pluginBlocked";
import { implementsTypes, isImporter, needsEntry } from "./pluginCapabilities";
import { collectExtensions, extensionLookup, setExtensionTable, type Contribution } from "./pluginExtensions";
import type { Provided } from "./pluginProvides";

export { acceptsKind, pluginKinds } from "./pluginKinds";
import { acceptsKind } from "./pluginKinds";

export interface LoadedExporter {
  manifest: PluginManifestDto;
  plugin: ExporterPlugin;
}
/** Importers are panel-only: registered from the manifest, their `importPanel`
 *  slot component does the work. */
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
  /** Every library type: native defaults overlaid with plugin declarations. */
  types: AssetTypeEntry[];
  errors: PluginLoadError[];
  /** Types switched off with their plugins: no section. */
  hiddenTypes: string[];
  /** Whether an item of `type`, imported by `originPlugin`, is hidden because
   *  its plugin is off. Hidden items are never deleted. */
  isHidden(type: string, originPlugin?: string): boolean;
  /** `isHidden` as lists, for queries (see `domain/browseQuery`). */
  rules: VisibilityRules;
  /** Every installed plugin, on or off, by name. Settings lists these. */
  plugins: PluginManifestDto[];
  /** What each loaded plugin's module turned out to provide, by plugin id. */
  provided: Record<string, Provided>;
  /** Contributions of the active plugins to an extension point. */
  extensions: ExtensionLookup;
}

export const emptyRegistry = (): PluginRegistry => ({ exporters: [], importers: [], types: defaultAssetTypes(), hiddenTypes: [], isHidden: () => false, rules: NO_RULES, errors: [], plugins: [], provided: {}, extensions: extensionLookup });

/** Discover + load all installed plugins. Each load is isolated: a throwing
 *  plugin becomes an `errors` entry, never aborting discovery. The extension
 *  table the plugins read is replaced once the load completes. */
export async function loadRegistry(): Promise<PluginRegistry> {
  const installed = await listPlugins();
  // Only active plugins load: one the user turned off, or whose dependencies
  // are not usable, is listed for settings but contributes nothing.
  const manifests = installed.filter(isActive);
  console.info(`[plugins] discovered ${installed.length}, ${manifests.length} active:`, manifests.map((m) => m.id));
  const registry: PluginRegistry = { exporters: [...nativeExporters], importers: [], types: [], ...pluginVisibility(installed), errors: [],
    plugins: [...installed].sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id)), provided: {}, extensions: extensionLookup };
  resetRigAdapters(BUILTIN_RIG_PLUGINS.filter((id) => installed.some((m) => m.id === id && !isActive(m))));
  for (const manifest of installed.filter(isBlocked)) registry.errors.push({ manifest, error: blockedSentence(manifest.blockedBy ?? [], installed) });
  const declared: DeclaredTypes[] = [];
  const contributions: Contribution[] = [];

  await Promise.all(
    manifests.map(async (manifest) => {
      if (isImporter(manifest)) registry.importers.push({ manifest });
      const declarations = manifest.assetTypes ?? [];
      const provided: Provided = { exporter: false, rigProfiles: false, extensionPoints: [] };
      let implementations: DeclaredTypes["implementations"];
      let failed = false;
      if (needsEntry(manifest)) {
        try {
          const mod = await loadPlugin(manifest);
          if (mod.run) {
            registry.exporters.push({ manifest, plugin: { run: mod.run, preview: mod.preview } });
            provided.exporter = true;
          }
          if (mod.profiles) {
            registerRigAdapters({ profiles: mod.profiles });
            provided.rigProfiles = mod.profiles.length > 0;
          }
          implementations = mod.types;
          if (mod.extensions !== undefined) contributions.push({ pluginId: manifest.id, extensions: mod.extensions });
        } catch (e) {
          failed = true;
          console.error(`[plugins] FAILED to load ${manifest.id}:`, e);
          registry.errors.push({ manifest, error: String(e) });
        }
      }
      if (!failed && implementsTypes(manifest) && !implementations) {
        registry.errors.push({ manifest, error: `plugin '${manifest.id}' declares types it implements itself, so its entry module must provide "types"` });
      }
      if (declarations.length) declared.push({ pluginId: manifest.id, declarations, implementations });
      if (!failed) registry.provided[manifest.id] = provided;
      console.info(`[plugins] registered ${manifest.id} (types: ${pluginKinds(manifest).join(", ") || "none"})`);
    }),
  );

  const { table, problems: extensionProblems } = collectExtensions(contributions);
  for (const [point, list] of table) for (const pluginId of new Set(list.map((entry) => entry.pluginId))) {
    registry.provided[pluginId]?.extensionPoints.push(point);
  }
  const { types, problems } = buildAssetTypes(declared, registry.hiddenTypes);
  registry.types = types;
  for (const problem of [...problems, ...extensionProblems]) {
    const manifest = manifests.find((m) => m.id === problem.pluginId);
    if (manifest) registry.errors.push({ manifest, error: problem.message });
  }
  setExtensionTable(table);
  return registry;
}

/** The importers that produce `kind`. */
export function importersFor(importers: LoadedImporter[], kind: AssetKind): LoadedImporter[] {
  return importers.filter((i) => acceptsKind(i.manifest, kind));
}

/** Run one exporter plugin over the current selection and return its report. */
export async function runExporter(
  loaded: LoadedExporter,
  selectedIds: string[],
  config: PluginConfig,
  signal: AbortSignal,
): Promise<ExportReport> {
  const ctx = buildExportCtx(loaded.manifest, config, signal);
  const assets = await loadSelectedAssets(selectedIds);
  if (assets.length !== selectedIds.length || assets.some((a) => !acceptsKind(loaded.manifest, assetType(a)))) {
    throw new Error("This exporter does not accept the selected asset type.");
  }
  return loaded.plugin.run(ctx, assets);
}
