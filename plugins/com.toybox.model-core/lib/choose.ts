/**
 * Which of the model files found should be imported: files that are levels of
 * detail of one model are grouped under the model's name, and either every
 * level or only the best one is taken. Pure.
 */

import { variantInfo } from "@ldlework/toybox-sdk";

import type { Found } from "./walk.js";

/** "All levels" imports every level of detail of a model; "best only" the first. */
export type LevelPolicy = "all" | "best";

/** The found files grouped by model name, each group best level first. */
export function groupModels(found: Found[]): Map<string, Found[]> {
  const groups = new Map<string, Found[]>();
  for (const file of found) {
    const info = variantInfo(file.stem, "model");
    const group = groups.get(info.base) ?? [];
    group.push(file);
    groups.set(info.base, group);
  }
  for (const group of groups.values()) group.sort((a, b) => variantInfo(a.stem, "model").rank - variantInfo(b.stem, "model").rank);
  return groups;
}

/** The files to import under `policy`, leaving out `excluded` (by `relPath`). */
export function chooseModels(groups: Map<string, Found[]>, policy: LevelPolicy, excluded: ReadonlySet<string> = new Set()): Found[] {
  return [...groups.values()].flatMap((group) => (policy === "best" ? group.slice(0, 1) : group).filter((f) => !excluded.has(f.relPath)));
}
