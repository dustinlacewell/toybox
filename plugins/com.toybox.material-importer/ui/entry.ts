/**
 * Draft -> library manifest entry. Pure: turns a material draft's file uses into
 * the map list a material entry records, or the reasons it cannot be imported.
 */

import type { MapChannel, MapRole, MaterialMapInput, MaterialSeedEntry } from "@ldlework/toybox-sdk";
import type { ChannelRole, DraftFile, MaterialDraft } from "./draft.js";

const CHANNELS: MapChannel[] = ["r", "g", "b", "a"];

/** The files that contribute at least one map — the ones to copy into the library. */
export function usedFiles(draft: MaterialDraft): DraftFile[] {
  return draft.files.filter((f) => mapsOf(f).length > 0);
}

export function toEntry(
  draft: MaterialDraft,
  pack: string,
  category: string,
): { entry: MaterialSeedEntry } | { problems: string[] } {
  const maps = draft.files.flatMap(mapsOf);
  const problems = [...nameProblems(draft), ...mapProblems(maps)];
  if (problems.length > 0) return { problems };

  return {
    entry: {
      kind: "material",
      id: `uid://import_${pack}_material_${draft.name}`,
      pack,
      category,
      name: draft.name,
      maps,
      flipNormalGreen: flipsGreen(draft),
    },
  };
}

/** The maps one file contributes; empty for an unassigned file. */
function mapsOf(file: DraftFile): MaterialMapInput[] {
  const { use } = file;
  switch (use.use) {
    case "unassigned":
      return [];
    case "channels":
      return CHANNELS.filter((c) => use.channels[c]).map((c) =>
        channelMap(file.fileName, c, use.channels[c]!),
      );
    default:
      return [{ role: use.use, file: file.fileName }];
  }
}

function channelMap(file: string, channel: MapChannel, role: ChannelRole): MaterialMapInput {
  return role === "smoothness"
    ? { role: "roughness", file, channel, invert: true }
    : { role, file, channel };
}

function flipsGreen(draft: MaterialDraft): boolean {
  return draft.files.some((f) => f.use.use === "normal" && f.use.directX);
}

function nameProblems(draft: MaterialDraft): string[] {
  const seen = new Set<string>();
  const dupes = new Set<string>();
  for (const f of draft.files) (seen.has(f.fileName) ? dupes : seen).add(f.fileName);
  return [...dupes].map((n) => `file name used twice: ${n}`);
}

function mapProblems(maps: MaterialMapInput[]): string[] {
  const counts = new Map<MapRole, number>();
  for (const m of maps) counts.set(m.role, (counts.get(m.role) ?? 0) + 1);
  const problems: string[] = [];
  if (!counts.has("albedo")) problems.push("no albedo map");
  for (const [role, n] of counts) if (n > 1) problems.push(`${role} assigned twice`);
  return problems;
}
