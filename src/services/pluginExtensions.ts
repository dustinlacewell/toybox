/**
 * Extension points. A plugin's entry module may contribute values to named
 * points; another plugin reads them with `ctx.extensions(point)`. The host only
 * carries the values: it never looks inside them. The table of the latest
 * successful load is kept here, and every plugin context gets the same stable
 * lookup, so a function a plugin captured keeps seeing the current plugins.
 * Pure: no host imports.
 */

import type { Extension, ExtensionLookup } from "@ldlework/toybox-sdk";

export type ExtensionTable = ReadonlyMap<string, Extension[]>;

export interface Contribution {
  pluginId: string;
  /** `module.extensions` as the plugin exported it, unchecked. */
  extensions: unknown;
}

const isRecord = (value: unknown): value is Record<string, unknown> => !!value && typeof value === "object" && !Array.isArray(value);

/** Every contribution of the given plugins, per point, ordered by plugin id
 *  then by the order within the plugin. A point whose value is not an array is
 *  skipped and reported against its plugin; the rest of that plugin's points
 *  and every other plugin are unaffected. */
export function collectExtensions(contributions: Contribution[]): { table: ExtensionTable; problems: { pluginId: string; message: string }[] } {
  const table = new Map<string, Extension[]>();
  const problems: { pluginId: string; message: string }[] = [];
  for (const { pluginId, extensions } of [...contributions].sort((a, b) => a.pluginId.localeCompare(b.pluginId))) {
    if (extensions === undefined) continue;
    if (!isRecord(extensions)) { problems.push({ pluginId, message: "its extensions must be an object keyed by extension point" }); continue; }
    for (const [point, values] of Object.entries(extensions)) {
      if (!Array.isArray(values)) { problems.push({ pluginId, message: `its contribution to '${point}' must be an array` }); continue; }
      if (!values.length) continue;
      const list = table.get(point) ?? [];
      for (const value of values) list.push({ pluginId, value });
      table.set(point, list);
    }
  }
  return { table, problems };
}

let current: ExtensionTable = new Map();

/** Replace the table every plugin reads. Called after a load completes. */
export const setExtensionTable = (table: ExtensionTable): void => { current = table; };

/** The lookup handed to every plugin context. It reads the current table when
 *  called, never a snapshot taken when the context was built. */
export const extensionLookup: ExtensionLookup = <T = unknown>(point: string): Extension<T>[] => [...(current.get(point) ?? [])] as Extension<T>[];
