/**
 * Rendering an existing Substance material again: with another preset, other
 * parameter values or another size, either in place or as a new material. The
 * asset panel's "Update this material" / "Save as new" and `tbx sbsar render`
 * both call this; `tbx sbsar show` reads the same record.
 */

import { importNamespace, type AssetView, type MaterialSeedEntry } from "@ldlework/toybox-sdk";

import { absolutePath, archiveFile, locateArchive } from "./archive.js";
import type { Env, Substance } from "./engine.js";
import { cleanName, materialId, presetValues, readMeta, sizeOf, type SbsarMeta, type Values } from "./graph.js";
import { applyParameters, checkPreset, type RawValue } from "./params.js";
import { produceMaterial } from "./produce.js";

/** The plugin that makes these materials (the host stamps it on `origin.plugin`). */
export const SBSAR_PLUGIN = "com.toybox.material-sbsar";

/** What a stored material was rendered with, or why it is not a Substance material of this plugin. */
export function renderedWith(asset: Pick<AssetView, "origin" | "kind">): { meta: SbsarMeta } | { reason: string } {
  if (asset.kind !== "material") return { reason: "it is not a material" };
  if (asset.origin?.plugin !== SBSAR_PLUGIN) return { reason: "it was not rendered from a Substance archive by this plugin" };
  const meta = readMeta(asset.origin.meta);
  return meta ? { meta } : { reason: "it records no Substance archive" };
}

export interface RerenderRequest {
  /** `update`: replace the material, keeping its id (so tags, collections and details follow it). `new`: add another. */
  mode: "update" | "new";
  /** The new material's name, for `new`. */
  name?: string;
  preset: string | null;
  values: Values;
  /** The texture size as the engine counts it (a power of two). */
  size: number;
}

/** Render `substance` (the material's archive, already loaded) as `request` says and stage the result. The entry is
 *  ready to commit. An update keeps the item's id; a new material copies its source details. */
export async function rerenderMaterial(env: Env, libraryRoot: string, asset: AssetView, meta: SbsarMeta, substance: Substance, request: RerenderRequest): Promise<MaterialSeedEntry> {
  const archive = locateArchive(asset, meta, libraryRoot);
  if (!archive) throw new Error("this material has no Substance archive stored with it");
  const file = absolutePath(archive);
  const update = request.mode === "update";
  const name = update ? asset.name : cleanName(request.name ?? "");
  return produceMaterial(env, substance, libraryRoot, {
    id: update ? asset.id : materialId(importNamespace(file), name),
    pack: importNamespace(`${file}#${Date.now()}`),
    category: asset.category,
    name,
    archive,
    archiveHash: meta.archiveHash,
    preset: request.preset,
    values: request.values,
    size: sizeOf(request.size),
    // A replaced material keeps its source details; a new one copies them.
    ...(!update && asset.user.provenance ? { provenance: asset.user.provenance } : {}),
  });
}

export interface RenderChoices {
  /** Use this preset instead of the material's. */
  preset?: string;
  /** Start from the archive's defaults instead of the material's preset and parameters. */
  reset?: boolean;
  /** Parameter values given as text or JSON, laid over the starting values. */
  given?: Record<string, RawValue>;
  ignoreMissing?: boolean;
  /** The texture size as the engine counts it (a power of two); the material's own when absent. */
  size?: number;
}

export interface RenderPlan {
  preset: string | null;
  values: Values;
  size: number;
  /** Given parameters the archive lacks and `ignoreMissing` left out. */
  ignored: string[];
}

/** What to render a stored material with: where it starts (the material's own preset and parameters, the archive's
 *  defaults with `reset`, or the chosen preset), with the given parameters on top. Throws, saying what is wrong,
 *  when a preset or parameter does not fit the loaded archive. */
export function planRender(substance: Substance, meta: SbsarMeta, choices: RenderChoices): RenderPlan {
  const graph = substance.graph;
  if (choices.preset) checkPreset(graph, choices.preset);
  const preset = choices.preset ?? (choices.reset ? null : meta.preset);
  if (!choices.reset && !choices.preset && meta.preset) checkPreset(graph, meta.preset);
  // The panel starts from the preset's values with the recorded changes on top; a chosen preset or a reset starts clean.
  const start: Values = choices.reset || choices.preset ? presetValues(graph, preset) : { ...presetValues(graph, preset), ...meta.inputs };
  const applied = applyParameters(graph, start, choices.given ?? {}, { ignoreMissing: choices.ignoreMissing });
  if (applied.problems.length) throw new Error(applied.problems.map((problem) => problem.message).join("; "));
  return { preset, values: applied.values, size: choices.size ?? meta.size, ignored: applied.ignored };
}
