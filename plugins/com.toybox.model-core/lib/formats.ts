/**
 * The model formats the importer understands: whatever other plugins contribute
 * to `model.formats`. This plugin handles no format itself. Pure, so it is
 * tested without a window.
 */

import type { ModelFormat, ModelSource, ModelTarget } from "../api.js";

/** One contribution as the host hands it over. */
export interface Contribution {
  pluginId: string;
  value: unknown;
}

/** The format that handles one extension, and the plugin it came from. */
export interface Claim {
  pluginId: string;
  format: ModelFormat;
}

/** Every extension the importer looks for, and who handles it. */
export interface FormatTable {
  /** Who handles each extension, lower case. */
  claims: Map<string, Claim>;
  /** The extensions to scan for, in the order claimed. Empty when no format is on. */
  extensions: string[];
}

/** What the import panel says when no plugin contributes a format. */
export const NO_FORMATS = "No model formats are turned on. Turn on a format plugin such as glTF models in Settings → Plugins.";

/** Whether anything can be imported. */
export const hasFormats = (table: FormatTable): boolean => table.extensions.length > 0;

export function isModelFormat(value: unknown): value is ModelFormat {
  const v = value as Partial<ModelFormat> | null;
  return !!v && typeof v === "object"
    && typeof v.id === "string" && v.id !== ""
    && typeof v.label === "string" && v.label !== ""
    && Array.isArray(v.extensions) && v.extensions.some((e) => typeof e === "string")
    && typeof v.import === "function";
}

/** Lower case without the dot; empty when nothing is left. */
export function normalizeExtension(ext: unknown): string {
  return typeof ext === "string" ? ext.trim().replace(/^\.+/, "").toLowerCase() : "";
}

/** Keep the well-formed contributions. When two plugins claim an extension the
 *  one with the lower plugin id wins. */
export function collectFormats(contributions: Contribution[]): FormatTable {
  const claims = new Map<string, Claim>();
  const ordered = [...contributions].sort((a, b) => (a.pluginId < b.pluginId ? -1 : a.pluginId > b.pluginId ? 1 : 0));
  for (const { pluginId, value } of ordered) {
    if (!isModelFormat(value)) continue;
    for (const raw of value.extensions) {
      const ext = normalizeExtension(raw);
      if (!ext || claims.has(ext)) continue;
      claims.set(ext, { pluginId, format: value });
    }
  }
  return { claims, extensions: [...claims.keys()] };
}

/** Lower-case extension of a file name, without the dot; empty when it has none. */
export function extensionOf(name: string): string {
  const i = name.lastIndexOf(".");
  return i <= 0 ? "" : name.slice(i + 1).toLowerCase();
}

/** The extension of `name` when the importer handles it, otherwise null. */
export function modelExtension(name: string, table: FormatTable): string | null {
  const ext = extensionOf(name);
  return ext && table.extensions.includes(ext) ? ext : null;
}

/** "Looks for .gltf, .glb, .fbx and .obj files." Says `NO_FORMATS` when none is on. */
export function lookingFor(table: FormatTable): string {
  if (!hasFormats(table)) return NO_FORMATS;
  const list = table.extensions.map((e) => `.${e}`);
  const text = list.length > 1 ? `${list.slice(0, -1).join(", ")} and ${list[list.length - 1]}` : list.join("");
  return `Looks for ${text} files.`;
}

/** An import as the importer uses it: the contract's result plus who made it. */
export type FormatImport = { file: string; warnings: string[]; originals: string[]; pluginId: string; keepsOriginal: boolean } | { rejected: string };

/** Run the plugin's importer for one file. A throw, or a reply that is neither
 *  an import nor a rejection, becomes a rejection that names the format. */
export async function importWithFormat(
  table: FormatTable,
  ext: string,
  source: ModelSource,
  target: ModelTarget,
): Promise<FormatImport> {
  const claim = table.claims.get(ext);
  if (!claim) return { rejected: `no importer for .${ext} files` };
  const { label } = claim.format;
  try {
    const result = await claim.format.import(source, target);
    if (result && typeof result === "object") {
      if ("rejected" in result && typeof result.rejected === "string") return { rejected: result.rejected };
      if ("file" in result && typeof result.file === "string" && result.file !== "") {
        const warnings = Array.isArray(result.warnings) ? result.warnings.filter((w): w is string => typeof w === "string") : [];
        const originals = Array.isArray(result.originals) ? result.originals.filter((p): p is string => typeof p === "string") : [];
        return { file: result.file, warnings, originals, pluginId: claim.pluginId, keepsOriginal: claim.format.keepsOriginal !== false };
      }
    }
    return { rejected: `${label} import returned no file` };
  } catch (e) {
    return { rejected: `${label} import failed: ${e instanceof Error ? e.message : String(e)}` };
  }
}
