/**
 * The Substances type of the library. The manifest declares it; `ui/Import`
 * brings archives in (each as a substance with a material rendered from it);
 * this module supplies a substance's viewer, thumbnail, tile subtitle and
 * properties. The materials are ordinary library materials that name their
 * substance in `origin.derivedFrom`; their Substance tab is `ui/AssetPanel`.
 * The command line has a module of its own (`cli.ts`), which never reaches this
 * one: the viewer needs React and a window.
 */

import { defineAssetTypes, type AssetProperty, type PluginAssetView } from "@ldlework/toybox-sdk";

import { renderThumbnail } from "./lib/thumbnail.js";
import { humanBytes, readSubstanceMeta, SUBSTANCE_TYPE, type SubstanceMeta } from "./lib/substance.js";
import { SubstanceViewer } from "./ui/Viewer.js";

const plural = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;

/** Totals over the graphs a substance records, or undefined when it records none. */
function totals(meta: SubstanceMeta | undefined) {
  const graphs = meta?.graphs;
  if (!graphs?.length) return undefined;
  return {
    graphs: graphs.length,
    presets: graphs.reduce((sum, graph) => sum + graph.presets.length, 0),
    parameters: graphs.reduce((sum, graph) => sum + graph.parameters, 0),
    outputs: [...new Set(graphs.flatMap((graph) => graph.outputs))],
  };
}

export function subtitle(asset: PluginAssetView): string {
  const known = totals(readSubstanceMeta(asset.meta));
  if (!known) return "Substance archive";
  return [...(known.graphs > 1 ? [plural(known.graphs, "graph")] : []), ...(known.presets ? [plural(known.presets, "preset")] : []), plural(known.parameters, "parameter")].join(" · ");
}

export function properties(asset: PluginAssetView): AssetProperty[] {
  const meta = readSubstanceMeta(asset.meta);
  if (!meta) return [];
  const known = totals(meta);
  return [
    { label: "Archive", value: meta.sbsar },
    ...(meta.bytes !== undefined ? [{ label: "Size on disk", value: humanBytes(meta.bytes) }] : []),
    ...(known ? [
      ...(known.graphs > 1 ? [{ label: "Graphs", value: known.graphs }] : []),
      { label: "Presets", value: known.presets || "None" },
      { label: "Parameters", value: known.parameters },
      { label: "Outputs", value: known.outputs.join(", ") || "None" },
    ] : []),
  ];
}

export default defineAssetTypes({
  types: {
    [SUBSTANCE_TYPE]: {
      Viewer: SubstanceViewer,
      // The viewer hands the archive to the Substance engine, so it must be on this computer.
      needsFiles: true,
      renderThumbnail,
      // 1: the material sphere in place of the flat swatch.
      thumbnailVersion: 1,
      subtitle,
      properties,
    },
  },
});
