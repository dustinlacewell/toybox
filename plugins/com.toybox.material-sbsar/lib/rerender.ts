/**
 * Rendering from a substance again: a material with another preset, other
 * parameter values or another size, either in place or as a new material, or a
 * substance into a new material of its own. The substance viewer and
 * `tbx sbsar render` both call this; `tbx sbsar show` and the material's
 * Substance tab read the same records.
 */

import { importNamespace, type AssetView, type MaterialSeedEntry } from "@ldlework/toybox-sdk";

import type { Env, Substance } from "./engine.js";
import { cleanName, materialId, presetValues, readMeta, sizeOf, type SbsarMeta, type Values } from "./graph.js";
import { applyParameters, checkPreset, type RawValue } from "./params.js";
import { produceMaterial } from "./produce.js";
import { baseName, isLegacyMaterial, isSubstance, rawId, readSubstanceMeta, SBSAR_PLUGIN, substanceArchive, type ArchiveRef } from "./substance.js";

export { SBSAR_PLUGIN } from "./substance.js";

/** What a stored material was rendered with, or why it is not a Substance material of this plugin. */
export function renderedWith(asset: Pick<AssetView, "origin" | "kind">): { meta: SbsarMeta } | { reason: string } {
  if (asset.kind !== "material") return { reason: "it is not a material" };
  if (asset.origin?.plugin !== SBSAR_PLUGIN) return { reason: "it was not rendered from a Substance archive by this plugin" };
  const meta = readMeta(asset.origin.meta);
  return meta ? { meta } : { reason: "it records no Substance archive" };
}

/** The substance a material was rendered from: its id, or why there is none to find. */
export function parentOf(asset: Pick<AssetView, "origin">): { id: string } | { reason: string } {
  const id = asset.origin?.derivedFrom;
  if (id) return { id };
  if (isLegacyMaterial(asset)) return { reason: "its library has not been converted to substances yet (open it once with this version of Toybox)" };
  return { reason: "it names no substance" };
}

/** A substance as far as rendering needs it. */
export type SubstanceAsset = Pick<AssetView, "id" | "name" | "category" | "user" | "kind"> & { type?: string; files?: string[]; meta?: unknown };

/** Where a substance's archive is under the library root, or why it cannot be found. */
export function locateSubstanceArchive(substance: SubstanceAsset | undefined, libraryRoot: string): ArchiveRef | { reason: string } {
  if (!substance || !isSubstance(substance)) return { reason: "its substance is no longer in the library" };
  const meta = readSubstanceMeta(substance.meta);
  const path = substanceArchive(substance);
  if (!meta || !path) return { reason: `its substance's archive${meta ? ` (${meta.sbsar})` : ""} is not stored with it` };
  return { root: libraryRoot, path };
}

/** The id of a material made from a substance as a new one (not by importing a folder): it follows the substance
 *  and the name, so saving the same name again replaces it. It cannot meet an import's ids, which follow the
 *  folder the archive was imported from (a path, never a `uid://substance_…` id). */
export function newMaterialId(substance: string, name: string): string {
  return materialId(importNamespace(rawId(substance)), cleanName(name));
}

/** The material of a substance that a new material named `name` would collide with: one of `siblings` (the
 *  substance's materials) with that name after cleaning, ignoring case, else `byId` (the item found under the id
 *  the new material would get), else none. */
export function takenBy<T extends Pick<AssetView, "id" | "name">>(name: string, siblings: (T & Pick<AssetView, "kind">)[], byId?: T): T | undefined {
  const clean = cleanName(name).toLowerCase();
  return siblings.find((item) => item.kind === "material" && cleanName(item.name).toLowerCase() === clean) ?? byId;
}

export interface RerenderRequest {
  /** `update`: replace `material`, keeping its id (so tags, collections and details follow it). `new`: add another. */
  mode: "update" | "new";
  /** The new material's name, for `new`. */
  name?: string;
  /** For `new`: the id of a material of the substance that this one replaces (the caller found the name taken and
   *  replacing was asked for). Without it the id follows the substance and the name. */
  replaceId?: string;
  preset: string | null;
  values: Values;
  /** The texture size as the engine counts it (a power of two). */
  size: number;
}

/** Render `loaded` (the substance's archive, already open in the engine) as `request` says and stage the result;
 *  the entry is ready to commit. An update replaces `material` and keeps its id and details; a new material copies
 *  its source details and category from `material` when there is one, else from the substance. */
export async function rerenderMaterial(env: Env, libraryRoot: string, source: { substance: SubstanceAsset; material?: AssetView }, loaded: Substance, request: RerenderRequest): Promise<MaterialSeedEntry> {
  const { substance, material } = source;
  const update = request.mode === "update";
  if (update && !material) throw new Error("there is no material to update");
  const name = update ? material!.name : cleanName(request.name ?? "");
  if (!update && !(request.name ?? "").trim()) throw new Error("a new material needs a name");
  const meta = readSubstanceMeta(substance.meta);
  const from = material ?? substance;
  const provenance = from.user?.provenance ?? substance.user?.provenance;
  const { entry } = await produceMaterial(env, loaded, libraryRoot, {
    id: update ? material!.id : request.replaceId ?? newMaterialId(substance.id, name),
    pack: importNamespace(`${rawId(substance.id)}#${Date.now()}`),
    category: from.category,
    name,
    derivedFrom: substance.id,
    sbsar: meta?.sbsar ?? baseName(substanceArchive(substance) ?? "archive.sbsar"),
    preset: request.preset,
    values: request.values,
    size: sizeOf(request.size),
    // A replaced material keeps its source details; a new one copies them.
    ...(!update && provenance ? { provenance } : {}),
  });
  return entry;
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

/** What to render with: where it starts (a material's own preset and parameters, the archive's defaults with
 *  `reset` or for a substance — `meta` undefined —, or the chosen preset), with the given parameters on top.
 *  Throws, saying what is wrong, when a preset or parameter does not fit the loaded archive. */
export function planRender(substance: Substance, meta: SbsarMeta | undefined, choices: RenderChoices, fallbackSize = 10): RenderPlan {
  const graph = substance.graph;
  if (choices.preset) checkPreset(graph, choices.preset);
  const own = meta && !choices.reset && !choices.preset;
  const preset = choices.preset ?? (own ? meta.preset : null);
  if (own && meta.preset) checkPreset(graph, meta.preset);
  // A material starts from its preset's values with the recorded changes on top; a chosen preset or a reset starts clean.
  const start: Values = own ? { ...presetValues(graph, preset), ...meta.inputs } : presetValues(graph, preset);
  const applied = applyParameters(graph, start, choices.given ?? {}, { ignoreMissing: choices.ignoreMissing });
  if (applied.problems.length) throw new Error(applied.problems.map((problem) => problem.message).join("; "));
  return { preset, values: applied.values, size: choices.size ?? meta?.size ?? fallbackSize, ignored: applied.ignored };
}
