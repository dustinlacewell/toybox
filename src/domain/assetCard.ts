/**
 * What the status card of one item shows, as plain data: its facts, its labeled values that filter and
 * the tags offered while one is typed. The card's components render these; the wording of each value is
 * the one the full view's properties use.
 */

import { variantInfo } from "@ldlework/toybox-sdk";

import type { Asset } from "./catalog";
import { importedFrom, provenanceValue, type ExcludableField } from "./facets";

/** What pressing a tile does: open the item's full view. */
export type StatAction = "open";

export interface Stat { id: string; value: string; caption: string; press?: StatAction }

export interface StatInput {
  asset: Asset;
  /** The item and its other detail levels or sizes. */
  variants: readonly Asset[];
  /** Nodes of the item's rig; undefined when it has none. */
  rigNodes?: number;
  /** A plugin type's own rows, already checked. */
  properties?: readonly { label: string; value: string | number }[];
}

/** Formatting the card takes from its caller (the design system owns time). */
export interface StatFormat { time: (seconds: number) => string }

/** Tiles shown at most. */
const MAX_STATS = 4;
/** A plugin type's own rows shown at most (the file count follows). */
const MAX_PLUGIN_ROWS = 3;

/** The item's facts, in order: at most four, none with an empty value. */
export function assetStats(input: StatInput, format: StatFormat): Stat[] {
  return statsOfKind(input, format).filter((stat) => stat.value !== "").slice(0, MAX_STATS);
}

/** How many there are, when there is more than one: a single level or size says nothing. */
function several(items: readonly unknown[]): string {
  return items.length > 1 ? String(items.length) : "";
}

function statsOfKind({ asset, variants, rigNodes, properties }: StatInput, format: StatFormat): Stat[] {
  switch (asset.kind) {
    case "model": return [
      { id: "levels", value: several(variants), caption: "Detail levels", press: "open" },
      { id: "rig-nodes", value: rigNodes === undefined ? "" : String(rigNodes), caption: "Bones" },
      { id: "origin", value: importedFrom(asset) ?? "", caption: "Origin" },
    ];
    case "material": return [
      { id: "maps", value: String(asset.material.maps.length), caption: "Maps", press: "open" },
      { id: "sizes", value: several(variants), caption: "Sizes", press: "open" },
      { id: "largest", value: variantInfo(asset.name, "material").label, caption: "Largest size" },
    ];
    case "audio": return [
      { id: "length", value: format.time(asset.audio.duration), caption: "Length", press: "open" },
      { id: "rate", value: sampleRateLabel(asset.audio.sampleRate), caption: "Sample rate" },
      { id: "channels", value: channelsLabel(asset.audio.channels), caption: "Channels" },
      { id: "format", value: asset.audio.format, caption: "Format" },
    ];
    case "animation": return [
      { id: "length", value: clipLengthLabel(asset.clip.duration), caption: "Length", press: "open" },
      { id: "rig", value: asset.clip.rig, caption: "Rig" },
      { id: "nodes", value: String(asset.clip.targetNodes.length), caption: "Animated bones" },
      { id: "source", value: asset.clip.sourceName, caption: "Source clip", press: "open" },
    ];
    case "plugin": return [
      ...(properties ?? []).filter((row) => String(row.value) !== "").slice(0, MAX_PLUGIN_ROWS)
        .map((row, index) => ({ id: `property:${index}`, value: String(row.value), caption: row.label })),
      { id: "files", value: String(asset.files.length), caption: "Files" },
    ];
  }
}

/** "Mono", "Stereo", else the channel count. */
export const channelsLabel = (channels: number): string => channels === 1 ? "Mono" : channels === 2 ? "Stereo" : String(channels);
/** "48 kHz". */
export const sampleRateLabel = (hertz: number): string => `${hertz / 1000} kHz`;
/** An animation clip's length: "1.25 s". */
export const clipLengthLabel = (seconds: number): string => `${seconds.toFixed(2)} s`;

/** The nodes of the first of the item's rigs that is known; undefined when none is. */
export function rigNodeCount(bindings: readonly { rigId: string }[] | undefined, rigs: readonly { id: string; nodes: readonly unknown[] }[] | undefined): number | undefined {
  for (const binding of bindings ?? []) {
    const rig = rigs?.find((candidate) => candidate.id === binding.rigId);
    if (rig) return rig.nodes.length;
  }
  return undefined;
}

export interface FacetValue { field: ExcludableField; label: string; value: string }

/** The item's labeled values that filter, in reading order: publisher, source, license, category. Empty ones are left out. */
export function assetFacets(asset: Asset): FacetValue[] {
  const values: FacetValue[] = [
    { field: "publishers", label: "Publisher", value: provenanceValue(asset, "publisher") },
    { field: "sources", label: "Source", value: provenanceValue(asset, "source") },
    { field: "licenses", label: "License", value: provenanceValue(asset, "license") },
    { field: "categories", label: "Category", value: asset.category ?? "" },
  ];
  return values.filter((entry) => entry.value !== "");
}

/** Tags found for typed text, without those the item has (any case), in the order found, at most `limit`. */
export function tagSuggestions<T extends { value: string }>(found: readonly T[] | undefined, have: readonly string[], limit = 8): T[] {
  const own = new Set(have.map((tag) => tag.toLowerCase()));
  return (found ?? []).filter((row) => !own.has(row.value.toLowerCase())).slice(0, limit);
}
