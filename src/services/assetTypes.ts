/**
 * The library's asset types. Every section of the library — Models, Audio,
 * Skyboxes — is one entry here, and entries come from plugin manifests
 * (`assetTypes`). A `native` type is one the host implements itself; a plugin
 * type is implemented by its plugin's entry module and uses the generic
 * plugin-asset shape.
 *
 * The four native kinds have built-in declarations so the library still works
 * when their plugins are missing; an installed plugin's declaration replaces
 * the matching default. Pure: no React, three or tauri imports.
 */

import type { AssetTypeDeclaration, AssetTypeImplementation } from "@ldlework/toybox-sdk";
import type { AssetKind, BuiltinKind } from "../domain/catalog";

export interface AssetTypeEntry {
  id: AssetKind;
  label: string;
  singular: string;
  /** Host-implemented (model, material, animation, audio). */
  native: boolean;
  /** The declaring plugin, absent for a built-in default. */
  pluginId?: string;
  /** Plugin-relative icon path, resolved by the caller. */
  icon?: string;
  tileAspectRatio: number;
  order: number;
  /** Loaded implementation of a plugin type. */
  implementation?: AssetTypeImplementation;
}

export const NATIVE_KINDS: readonly BuiltinKind[] = ["model", "material", "animation", "audio"];
const TYPE_ID = /^[a-z][a-z0-9-]{1,31}$/;
/** Ids a plugin type may not take: native kinds, the generic kind tag, and the
 *  managed storage folders. */
const RESERVED = new Set<string>([...NATIVE_KINDS, "plugin", "models", "materials", "animations"]);

const DEFAULTS: AssetTypeEntry[] = [
  { id: "model", label: "Models", singular: "Model", native: true, tileAspectRatio: 1, order: 10 },
  { id: "material", label: "Materials", singular: "Material", native: true, tileAspectRatio: 1, order: 20 },
  { id: "animation", label: "Animations", singular: "Animation", native: true, tileAspectRatio: 1, order: 30 },
  { id: "audio", label: "Audio", singular: "Audio", native: true, tileAspectRatio: 2, order: 40 },
];

export const isNativeKind = (id: string): id is BuiltinKind => (NATIVE_KINDS as readonly string[]).includes(id);

/** Why a declaration cannot be registered, or undefined when it is valid. */
export function declarationProblem(declaration: AssetTypeDeclaration): string | undefined {
  if (!declaration || typeof declaration.id !== "string" || typeof declaration.label !== "string" || !declaration.label.trim()) {
    return "an asset type needs an id and a label";
  }
  const native = declaration.implementation === "native";
  if (native) return isNativeKind(declaration.id) ? undefined : `'${declaration.id}' is not a type the host implements`;
  if (!TYPE_ID.test(declaration.id)) return `'${declaration.id}' is not a valid type id (lower-case letters, digits and dashes)`;
  if (RESERVED.has(declaration.id)) return `'${declaration.id}' is reserved`;
  return undefined;
}

export interface DeclaredTypes {
  pluginId: string;
  declarations: AssetTypeDeclaration[];
  implementations?: Record<string, AssetTypeImplementation>;
}

/** Merge plugin declarations over the built-in defaults. The first plugin (by
 *  id) to declare a type owns it; later duplicates are reported, not applied. */
export function buildAssetTypes(declared: DeclaredTypes[], hidden: Iterable<string> = []): { types: AssetTypeEntry[]; problems: { pluginId: string; message: string }[] } {
  const byId = new Map<string, AssetTypeEntry>(DEFAULTS.map((entry) => [entry.id, { ...entry }]));
  const problems: { pluginId: string; message: string }[] = [];
  for (const { pluginId, declarations, implementations } of [...declared].sort((a, b) => a.pluginId.localeCompare(b.pluginId))) {
    for (const declaration of declarations) {
      const problem = declarationProblem(declaration);
      if (problem) { problems.push({ pluginId, message: problem }); continue; }
      const existing = byId.get(declaration.id);
      if (existing?.pluginId) {
        problems.push({ pluginId, message: `type '${declaration.id}' is already defined by ${existing.pluginId}` });
        continue;
      }
      const native = declaration.implementation === "native";
      byId.set(declaration.id, {
        id: declaration.id,
        label: declaration.label.trim(),
        singular: declaration.singular?.trim() || declaration.label.trim(),
        native,
        pluginId,
        icon: declaration.icon,
        tileAspectRatio: positive(declaration.tileAspectRatio) ?? existing?.tileAspectRatio ?? 1,
        order: Number.isFinite(declaration.order) ? Number(declaration.order) : existing?.order ?? 100,
        implementation: native ? undefined : implementations?.[declaration.id],
      });
    }
  }
  // A switched-off type has no section, built-in default included.
  for (const id of hidden) if (!byId.get(id)?.pluginId) byId.delete(id);
  const types = [...byId.values()].sort((a, b) => a.order - b.order || a.label.localeCompare(b.label));
  return { types, problems };
}

const positive = (value: unknown) => (typeof value === "number" && Number.isFinite(value) && value > 0 ? value : undefined);

export const defaultAssetTypes = (): AssetTypeEntry[] => buildAssetTypes([]).types;

/** A plugin takes part when it is switched on and nothing it needs is missing
 *  (the rule of `pluginActive`, restated: this module has no runtime imports). */
const isActive = (plugin: { enabled?: boolean; blockedBy?: unknown[] }) => plugin.enabled !== false && !plugin.blockedBy?.length;

/** The types that are switched off: declared by a plugin that is not active
 *  and by no plugin that is. Their sections and items are hidden — nothing
 *  is removed from the library, and turning the plugin back on shows them. */
export function disabledAssetTypes(plugins: { enabled?: boolean; blockedBy?: unknown[]; assetTypes?: AssetTypeDeclaration[] }[]): string[] {
  const ids = (on: boolean) => plugins
    .filter((plugin) => isActive(plugin) === on)
    .flatMap((plugin) => (plugin.assetTypes ?? []).map((type) => type?.id))
    .filter((id): id is string => typeof id === "string");
  const provided = new Set(ids(true));
  return [...new Set(ids(false))].filter((id) => !provided.has(id)).sort();
}

/** What turning plugins off hides. Several plugins can put items in one type —
 *  a folder importer and a generator both make materials — so hiding follows
 *  the plugin an item came from, not only its type:
 *
 *  - an item imported by a plugin that is off is hidden;
 *  - an item with no recorded plugin belongs to the plugin that declares its
 *    type, and is hidden when that one is off;
 *  - a type's section stays while a plugin that is on declares it, or — for a
 *    type the host implements — still imports into it;
 *  - an item that records a retired plugin id follows the plugins that replaced
 *    it (their manifests list the id in `replaces`): it is hidden when none of
 *    them is on. A retired id nothing replaced falls back to the type's owner.
 *
 *  Nothing is deleted; everything returns with its plugin. */
export interface PluginVisibility {
  /** Types with no section. */
  hiddenTypes: string[];
  isHidden(type: string, originPlugin?: string): boolean;
  /** The same rule as lists, for the backend to hide items in a query (see `domain/browseQuery`). */
  rules: {
    hiddenTypes: string[];
    pluginsOff: string[];
    pluginsOn: string[];
    retiredOn: string[];
    retiredOff: string[];
    ownerOff: string[];
  };
}

interface VisibilitySource { id: string; enabled?: boolean; blockedBy?: unknown[]; replaces?: string[]; assetKinds?: string[]; assetTypes?: AssetTypeDeclaration[]; ui?: { importPanel?: string } }

/** The installed plugins that replaced the retired plugin id `retired`, by id.
 *  Library items keep the id they were imported under; this is how they find
 *  the plugins that carry on its work. Empty for an id that was never retired
 *  or was retired without a successor. */
export function replacementsOf(plugins: { id: string; replaces?: string[] }[], retired: string): string[] {
  return plugins.filter((plugin) => plugin.replaces?.includes(retired)).map((plugin) => plugin.id).sort();
}

/** The id an exporter is known by now. A saved export names its exporter by
 *  plugin id; when that plugin was retired the export follows the one plugin that
 *  took over (an id that is installed, that nothing replaced, or that more than
 *  one plugin replaced stays as it is). */
export function resolveExporterId(exporters: { id: string; replaces?: string[] }[], id: string): string {
  if (!id || exporters.some((exporter) => exporter.id === id)) return id;
  const successors = replacementsOf(exporters, id);
  return successors.length === 1 ? successors[0] : id;
}

export function pluginVisibility(plugins: VisibilitySource[]): PluginVisibility {
  const ownerOff = new Set(disabledAssetTypes(plugins));
  const enabled = new Map(plugins.map((plugin) => [plugin.id, isActive(plugin)]));
  // Native types an enabled plugin still imports into, without declaring them.
  const contributed = new Set<string>(plugins
    .filter((plugin) => isActive(plugin) && (plugin.assetTypes?.length || plugin.ui?.importPanel))
    // No runtime imports here (see the header), so the rule of `pluginKinds`
    // is restated: what a plugin lists, or models when it lists nothing.
    .flatMap((plugin) => plugin.assetKinds ?? (plugin.assetTypes?.length ? [] : ["model"]))
    .filter((kind) => isNativeKind(kind)));
  const hidden = new Set([...ownerOff].filter((type) => !contributed.has(type)));
  const installed = new Set(plugins.map((plugin) => plugin.id));
  const retired = [...new Set(plugins.flatMap((plugin) => plugin.replaces ?? []))].filter((id) => !installed.has(id) && replacementsOf(plugins, id).length);
  const rules = {
    hiddenTypes: [...hidden].sort(),
    pluginsOff: plugins.filter((plugin) => !enabled.get(plugin.id)).map((plugin) => plugin.id).sort(),
    pluginsOn: plugins.filter((plugin) => enabled.get(plugin.id)).map((plugin) => plugin.id).sort(),
    retiredOn: retired.filter((id) => replacementsOf(plugins, id).some((successor) => enabled.get(successor))).sort(),
    retiredOff: retired.filter((id) => !replacementsOf(plugins, id).some((successor) => enabled.get(successor))).sort(),
    ownerOff: [...ownerOff].sort(),
  };
  return {
    rules,
    hiddenTypes: [...hidden].sort(),
    isHidden(type, originPlugin) {
      if (hidden.has(type)) return true;
      const origin = originPlugin !== undefined ? enabled.get(originPlugin) : undefined;
      if (origin !== undefined) return !origin;
      const successors = originPlugin !== undefined ? replacementsOf(plugins, originPlugin) : [];
      if (successors.length) return !successors.some((id) => enabled.get(id));
      return ownerOff.has(type);
    },
  };
}

/** A stand-in for a type whose plugin is not installed, so its assets still
 *  list, filter and export. */
export function unknownAssetType(id: string): AssetTypeEntry {
  const label = id.charAt(0).toUpperCase() + id.slice(1);
  return { id, label, singular: label, native: false, tileAspectRatio: 1, order: 1000 };
}

/** Registered types, plus a stand-in for every type that has assets but no
 *  installed plugin. Types in `hidden` (switched off) get no stand-in. */
export function withAssetTypesInUse(types: AssetTypeEntry[], inUse: Iterable<string>, hidden: Iterable<string> = []): AssetTypeEntry[] {
  const known = new Set([...types.map((entry) => entry.id), ...hidden]);
  const extra = [...new Set(inUse)].filter((id) => !known.has(id)).sort().map(unknownAssetType);
  return extra.length ? [...types, ...extra] : types;
}
