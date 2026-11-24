/**
 * Words and data about archives and the materials made from them, for people
 * and for programs: what `tbx sbsar inputs`, `show` and `render --dry-run` print.
 */

import type { AssetView } from "@ldlework/toybox-sdk";

import { editableInputs, presetValues, rangeOf, readMeta, sizeOf, toHex, type Graph, type SbsarMeta, type Value, type Values } from "./graph.js";
import { parentOf, renderedWith, type SubstanceAsset } from "./rerender.js";
import { humanBytes, isSubstance, readSubstanceMeta, substanceArchive } from "./substance.js";

const trim = (value: number) => Number(value.toFixed(4));

/** A value as text, the way `--set` takes it. */
export function showValue(value: Value): string {
  return Array.isArray(value) ? value.map(trim).join(",") : typeof value === "number" ? String(trim(value)) : value;
}

/** The pixel size an engine size stands for. */
export const pixels = (size: number): number => 2 ** sizeOf(size);

/** One graph of an archive as data: its outputs, presets and the parameters a person can set. With `preset`, each
 *  parameter also carries the `value` that preset gives it. */
export function describeGraph(graph: Graph, preset?: string | null) {
  const values: Values | undefined = preset ? presetValues(graph, preset) : undefined;
  const inputs = editableInputs(graph).map((input) => {
    const numeric = /^(float|integer)$/.test(input.type) && input.guiWidget !== "Combobox" && input.guiWidget !== "Togglebutton";
    const toggle = input.guiWidget === "Togglebutton";
    const colour = /^float[34]$/.test(input.type) && input.guiWidget === "Color";
    return {
      identifier: input.identifier,
      label: input.label,
      type: input.type,
      group: input.guiGroup,
      default: input.defaultValue,
      ...(values ? { value: values[input.identifier] ?? input.defaultValue } : {}),
      ...(numeric && !input.identifier.startsWith("$") ? { range: rangeOf(input) } : {}),
      ...(input.enumValues.length ? { choices: input.enumValues } : {}),
      ...(toggle ? { switch: true } : {}),
      ...(colour ? { colour: true } : {}),
    };
  });
  return {
    index: graph.index,
    label: graph.label,
    outputs: graph.outputs.map((output) => ({ identifier: output.identifier, usage: output.usage })),
    presets: graph.presets.map((item) => item.label),
    ...(preset ? { preset } : {}),
    inputs,
  };
}

export type GraphDescription = ReturnType<typeof describeGraph>;

/** The text `tbx sbsar inputs` prints for one graph. */
export function graphText(graph: GraphDescription): string {
  const lines = [`${graph.label} (graph ${graph.index})`];
  lines.push(`  outputs: ${graph.outputs.map((output) => (output.usage && output.usage !== output.identifier ? `${output.identifier} (${output.usage})` : output.identifier)).join(", ") || "none"}`);
  lines.push(`  presets: ${graph.presets.join(", ") || "none"}`);
  lines.push(graph.preset ? `  parameters, with the values of the preset "${graph.preset}":` : "  parameters:");
  if (!graph.inputs.length) lines.push("    none");
  for (const input of graph.inputs) {
    const detail: string[] = [];
    if ("value" in input && input.value !== undefined) detail.push(`value ${showValue(input.value)}`);
    detail.push(`default ${showValue(input.default)}`);
    if ("range" in input && input.range) detail.push(`range ${input.range.min}..${input.range.max}`);
    if ("choices" in input && input.choices) detail.push(`choices ${input.choices.map((choice) => `${choice.value}=${choice.label}`).join(", ")}`);
    if ("switch" in input) detail.push("true/false");
    if ("colour" in input) detail.push(`colour, also as ${toHex(input.default)}`);
    if (input.group) detail.push(`group ${input.group}`);
    lines.push(`    ${input.identifier}  ${input.type}  ${detail.join("; ")}`);
  }
  return lines.join("\n");
}

/** What a Substance material was rendered with, as data. `parent` is its substance as the library has it now
 *  (undefined when it is gone or the material names none). */
export function describeRendering(asset: Pick<AssetView, "id" | "name" | "origin" | "kind" | "library">, parent?: SubstanceAsset) {
  const found = renderedWith(asset);
  if ("reason" in found) return { id: asset.id, name: asset.name, library: asset.library, substance: false as const, reason: found.reason };
  const meta: SbsarMeta = found.meta;
  const link = parentOf(asset);
  const present = !!parent && isSubstance(parent);
  return {
    id: asset.id, name: asset.name, library: asset.library, substance: true as const,
    archive: meta.sbsar, graph: meta.graph, ...(meta.graphLabel ? { graphLabel: meta.graphLabel } : {}), preset: meta.preset,
    inputs: meta.inputs, size: meta.size, pixels: pixels(meta.size),
    archiveStored: present && !!substanceArchive(parent),
    madeFrom: "id" in link ? { id: link.id, ...(present ? { name: parent.name } : { missing: true as const }) } : { reason: link.reason },
  };
}

/** A substance as data: its archive, what is known of its graphs, and the materials made from it. */
export function describeSubstance(asset: SubstanceAsset & Pick<AssetView, "library">, materials: Pick<AssetView, "id" | "name" | "origin">[]) {
  const meta = readSubstanceMeta(asset.meta);
  return {
    id: asset.id, name: asset.name, library: asset.library, kind: "substance" as const,
    archive: meta?.sbsar ?? null, archiveStored: !!substanceArchive(asset), ...(meta?.bytes !== undefined ? { bytes: meta.bytes } : {}),
    ...(meta?.sha256 ? { sha256: meta.sha256 } : {}), ...(meta?.graphs ? { graphs: meta.graphs } : {}),
    materials: materials.map((material) => {
      const recorded = readMeta(material.origin?.meta);
      return { id: material.id, name: material.name, preset: recorded?.preset ?? null, size: recorded?.size ?? null, pixels: recorded ? pixels(recorded.size) : null };
    }).sort((a, b) => a.name.localeCompare(b.name)),
  };
}

/** The text `tbx sbsar show` prints for one substance. */
export function substanceText(item: ReturnType<typeof describeSubstance>): string {
  const lines = [`${item.id}  ${item.name}  (substance)`];
  lines.push(`  archive:    ${item.archive ?? "(none recorded)"}${item.archiveStored ? "" : " (not stored)"}${item.bytes !== undefined ? `, ${humanBytes(item.bytes)}` : ""}`);
  for (const graph of item.graphs ?? []) {
    lines.push(`  graph ${graph.index}:    ${graph.label}: ${graph.parameters} parameters; presets: ${graph.presets.join(", ") || "none"}; outputs: ${graph.outputs.join(", ") || "none"}`);
  }
  if (!item.graphs) lines.push("  graphs:     not recorded (import the archive again, or see: tbx sbsar inputs)");
  lines.push(`  materials:  ${item.materials.length || "none"}`);
  for (const material of item.materials) lines.push(`    ${material.name}  (${material.preset ?? "defaults"}${material.pixels ? `, ${material.pixels} px` : ""})  ${material.id}`);
  return lines.join("\n");
}

/** The changed parameters as `name=value` text. */
export function inputsText(inputs: Values): string {
  const names = Object.keys(inputs);
  return names.length ? names.map((name) => `${name}=${showValue(inputs[name])}`).join(", ") : "none";
}

/** The text `tbx sbsar show` prints for one material. */
export function renderingText(rendering: ReturnType<typeof describeRendering>): string {
  if (!rendering.substance) return `${rendering.id}  ${rendering.name}\n  not a Substance material: ${rendering.reason}`;
  return [
    `${rendering.id}  ${rendering.name}`,
    `  substance:  ${"reason" in rendering.madeFrom ? `none (${rendering.madeFrom.reason})` : "missing" in rendering.madeFrom ? `${rendering.madeFrom.id} (no longer in the library)` : `${rendering.madeFrom.name} (${rendering.madeFrom.id})`}`,
    `  archive:    ${rendering.archive}${rendering.archiveStored ? "" : " (not stored)"}`,
    `  graph:      ${rendering.graph}${rendering.graphLabel ? ` (${rendering.graphLabel})` : ""}`,
    `  preset:     ${rendering.preset ?? "none"}`,
    `  parameters: ${inputsText(rendering.inputs)}${Object.keys(rendering.inputs).length ? " (changed from the preset or defaults)" : ""}`,
    `  size:       ${rendering.pixels} px`,
  ].join("\n");
}

/** How a material was rendered, in a few words: `preset Green, count=7, 512 px` or `defaults, 1024 px`. */
export function shortRendering(meta: Pick<SbsarMeta, "preset" | "inputs" | "size">): string {
  const parts = [meta.preset ? `preset ${meta.preset}` : "defaults"];
  if (Object.keys(meta.inputs).length) parts.push(inputsText(meta.inputs));
  parts.push(`${pixels(meta.size)} px`);
  return parts.join(", ");
}
