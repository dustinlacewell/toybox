/**
 * Substances: the `substance` asset type this plugin declares. A substance is
 * one Substance archive (`.sbsar`) in the library; the materials rendered from
 * it point at it with `origin.derivedFrom`. This module is the one place that
 * names a substance, builds its entry, reads its record, stages its archive and
 * finds the archive again. UI-free and host-free, shared by the panels, the
 * viewer and `tbx`.
 *
 * - **identity**: `uid://substance_<first 32 hex digits of the archive's sha256>`,
 *   so one archive is one substance per library wherever it was imported from;
 * - **staged** by `stageSubstanceArchive`: copied to
 *   `library/<pack>/substance/<name>/<file>` (next to its `preview.png`), as
 *   any plugin-type entry is staged; at commit the host moves the folder into
 *   `library/substance/<key>/`;
 * - **found** by `substanceArchive`: the library-relative path in the
 *   substance's `files` whose last segment is `meta.sbsar` — never by position.
 */

import type { AssetView, FsApi, ImportProvenance, PluginSeedEntry } from "@ldlework/toybox-sdk";

import { editableInputs, type Graph } from "./graph.js";

/** The plugin that makes substances and their materials (the host stamps it on `origin.plugin`). */
export const SBSAR_PLUGIN = "com.toybox.material-sbsar";
/** The asset type id declared in the manifest. */
export const SUBSTANCE_TYPE = "substance";
/** The small picture of a substance kept next to its archive. */
export const PREVIEW_FILE = "preview.png";
/** The largest side of `preview.png`, in pixels (and as the engine counts it). */
export const PREVIEW_PIXELS = 512;
export const PREVIEW_SIZE = 9;

export const baseName = (path: string): string => path.replace(/\\/g, "/").split("/").pop() ?? path;

/** The raw id of an id the host may have qualified with its library (`<library>::<id>`). */
export function rawId(id: string): string {
  const at = id.indexOf("::");
  return at > 0 && /^[A-Za-z0-9_-]+$/.test(id.slice(0, at)) ? id.slice(at + 2) : id;
}

/** Whether two ids name the same item, qualified or not. */
export const sameId = (a: string | undefined, b: string | undefined): boolean => !!a && !!b && rawId(a) === rawId(b);

/** The substance id of an archive, from the sha256 of its content (64 hex digits). */
export function substanceId(sha256: string): string {
  const hex = sha256.trim().toLowerCase();
  if (!/^[0-9a-f]{32,}$/.test(hex)) throw new Error(`not a sha256: ${sha256}`);
  return `uid://substance_${hex.slice(0, 32)}`;
}

/** What one graph of an archive offers, in a few numbers: kept on the substance so the library can describe it
 *  without loading the archive. */
export interface GraphSummary {
  index: number;
  label: string;
  presets: string[];
  /** How many parameters a person can change. */
  parameters: number;
  /** What each output is used as (`baseColor`, `normal`, ...), or its identifier when the archive does not say. */
  outputs: string[];
}

/** What a substance records (`meta` of its entry). */
export interface SubstanceMeta {
  /** File name of the archive among the substance's files. */
  sbsar: string;
  /** SHA-256 of the archive, 64 hex digits. */
  sha256: string;
  /** Size of the archive in bytes, when known. */
  bytes?: number;
  /** The archive's graphs, when known (a library converted from the old layout has none until imported again). */
  graphs?: GraphSummary[];
}

export function summarizeGraphs(graphs: Graph[]): GraphSummary[] {
  return graphs.map((graph) => ({
    index: graph.index,
    label: graph.label,
    presets: graph.presets.map((preset) => preset.label),
    parameters: editableInputs(graph).length,
    outputs: graph.outputs.map((output) => output.usage || output.identifier),
  }));
}

const record = (value: unknown): Record<string, unknown> => (value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {});
const strings = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : []);
const count = (value: unknown): number | undefined => (typeof value === "number" && Number.isFinite(value) && value >= 0 ? Math.floor(value) : undefined);

/** A substance's `meta`, read defensively; undefined when it names no archive (or one with a path in it). */
export function readSubstanceMeta(meta: unknown): SubstanceMeta | undefined {
  const value = record(meta);
  if (typeof value.sbsar !== "string" || !value.sbsar || /[\\/]/.test(value.sbsar)) return undefined;
  const sha256 = typeof value.sha256 === "string" && /^[0-9a-f]{64}$/i.test(value.sha256) ? value.sha256.toLowerCase() : "";
  const bytes = count(value.bytes);
  const graphs = Array.isArray(value.graphs)
    ? value.graphs.map(record).filter((graph) => typeof graph.label === "string").map((graph, index) => ({
      index: count(graph.index) ?? index,
      label: graph.label as string,
      presets: strings(graph.presets),
      parameters: count(graph.parameters) ?? 0,
      outputs: strings(graph.outputs),
    }))
    : undefined;
  return { sbsar: value.sbsar, sha256, ...(bytes !== undefined ? { bytes } : {}), ...(graphs ? { graphs } : {}) };
}

/** Whether `asset` is a substance. */
export function isSubstance(asset: Pick<AssetView, "kind"> & { type?: string }): boolean {
  return asset.kind === "plugin" && asset.type === SUBSTANCE_TYPE;
}

/** The library-relative path of a substance's archive, or undefined when its record or files do not name one. */
export function substanceArchive(asset: { files?: string[]; meta?: unknown }): string | undefined {
  const meta = readSubstanceMeta(asset.meta);
  return meta ? (asset.files ?? []).find((file) => baseName(file) === meta.sbsar) : undefined;
}

/** Whether a substance lists its `preview.png`. */
export const hasPreview = (asset: { files?: string[] }): boolean => (asset.files ?? []).some((file) => baseName(file) === PREVIEW_FILE);

/** A file the plugin can read: a path under a folder it may read. */
export interface ArchiveRef {
  root: string;
  path: string;
}

/** The absolute path of a file under a root, with forward slashes (what the engine is told). */
export function absolutePath(ref: ArchiveRef): string {
  const root = ref.root.replace(/\\/g, "/");
  return `${root}${root.endsWith("/") ? "" : "/"}${ref.path.replace(/\\/g, "/")}`;
}

/** Where a substance's files are staged before commit: `library/<pack>/substance/<name>/`. */
export const substanceStaging = (pack: string, name: string): string => `library/${pack}/${SUBSTANCE_TYPE}/${name}`;

/** Copy an archive into a substance's staging folder; returns its bare file name. */
export async function stageSubstanceArchive(fs: Pick<FsApi, "copyFile">, libraryRoot: string, pack: string, name: string, from: ArchiveRef): Promise<string> {
  const file = baseName(from.path);
  await fs.copyFile(from.root, from.path, libraryRoot, `${substanceStaging(pack, name)}/${file}`);
  return file;
}

export interface SubstanceRequest {
  id: string;
  /** Staging namespace (see `substanceStaging`). */
  pack: string;
  category: string;
  name: string;
  /** Bare file name of the staged archive. */
  sbsar: string;
  sha256: string;
  bytes?: number;
  graphs?: Graph[];
  /** `preview.png` was staged next to the archive. */
  preview: boolean;
  provenance?: ImportProvenance;
  tags?: string[];
}

/** The entry that adds a substance to the library. */
export function substanceEntry(request: SubstanceRequest): PluginSeedEntry {
  const meta: SubstanceMeta = {
    sbsar: request.sbsar, sha256: request.sha256,
    ...(request.bytes !== undefined ? { bytes: request.bytes } : {}),
    ...(request.graphs?.length ? { graphs: summarizeGraphs(request.graphs) } : {}),
  };
  return {
    kind: "plugin", type: SUBSTANCE_TYPE, id: request.id, pack: request.pack, category: request.category, name: request.name, format: "sbsar",
    files: request.preview ? [request.sbsar, PREVIEW_FILE] : [request.sbsar],
    meta: { ...meta },
    ...(request.provenance ? { provenance: request.provenance } : {}),
    ...(request.tags?.length ? { tags: request.tags } : {}),
  };
}

/** A material of the layout before substances: it keeps its archive itself (`origin.files`) and names no
 *  substance. The library converts such materials when it is next opened. */
export function isLegacyMaterial(asset: Pick<AssetView, "origin">): boolean {
  return !asset.origin?.derivedFrom && (asset.origin?.files ?? []).some((file) => /\.sbsar$/i.test(file));
}

/** Bytes as a person reads them: `246.6 MB`. */
export function humanBytes(bytes: number): string {
  const units = ["bytes", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
  return unit === 0 ? `${bytes} bytes` : `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}
