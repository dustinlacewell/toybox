/**
 * Keeping the original file of a converted model. Pure planning: which staged
 * names the original and its companions get beside the converted glTF, and the
 * `origin` record the entry commits. The file copying lives in `walk.ts`.
 *
 * Names are staged exactly as before. At commit the host moves each kept file into
 * shared storage by content (`library/models/_sources/<sha256>/<name>`), so a companion
 * that several models share (a `.mtl`, a texture next to an `.fbx`) is stored once, and
 * `asset.origin.files` lists those library-relative paths in the order planned here
 * (the source first). The last segment of each is the name chosen here.
 */

import type { SeedOrigin } from "@ldlework/toybox-sdk";

/** Names already taken in the folder the importer stages into, across one import. */
export interface StagedNames {
  /** Lower-case names of converted files (glTF, bin, images) of models imported so far. */
  converted: Set<string>;
  /** Lower-case names given to originals so far. */
  originals: Set<string>;
  /** The name a source path was given, so a file two models share is staged once. */
  byPath: Map<string, string>;
}

export function newStagedNames(): StagedNames {
  return { converted: new Set(), originals: new Set(), byPath: new Map() };
}

/** One file to copy into the staging folder. */
export interface StagedFile {
  /** Path relative to the source root. */
  from: string;
  /** Bare name in the staging folder (and, later, in the model's own folder). */
  name: string;
}

export interface OriginalPlan {
  /** The source file first, then its companions. */
  files: StagedFile[];
  /** The record to commit, without `plugin` (the contributing plugin's id is added by the caller). */
  origin: SeedOrigin;
}

const baseOf = (path: string): string => path.slice(path.lastIndexOf("/") + 1);
const dirOf = (path: string): string => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");

/** A free name, adding `_2`, `_3`... before the extension when taken. */
function freeName(wanted: string, taken: (lower: string) => boolean): string {
  const dot = wanted.lastIndexOf(".");
  const stem = dot > 0 ? wanted.slice(0, dot) : wanted;
  const ext = dot > 0 ? wanted.slice(dot) : "";
  let name = wanted;
  for (let n = 2; taken(name.toLowerCase()); n++) name = `${stem}_${n}${ext}`;
  return name;
}

/** Where a companion goes relative to the original's folder when the original is
 *  exported again: the same relative path when it lies inside that folder,
 *  otherwise just its name (nothing is ever placed above the original). */
export function companionPlacement(sourcePath: string, companion: string): string {
  const dir = dirOf(sourcePath);
  if (dir === "") return companion;
  return companion.startsWith(`${dir}/`) ? companion.slice(dir.length + 1) : baseOf(companion);
}

/** Clean a list of companion paths a format returned: forward slashes, relative,
 *  no `..`, no repeats, never the source itself. */
export function cleanCompanions(paths: unknown, source: string): string[] {
  if (!Array.isArray(paths)) return [];
  const out: string[] = [];
  for (const raw of paths) {
    if (typeof raw !== "string") continue;
    const path = raw.replace(/\\/g, "/").split("/").filter((s) => s !== "" && s !== ".").join("/");
    if (!path || path.includes(":") || path.split("/").includes("..") || path === source || out.includes(path)) continue;
    out.push(path);
  }
  return out;
}

/**
 * Plan the original of one model. `format` is the extension recorded as the
 * entry's `format`; `source` and `companions` are paths relative to the source
 * root; `converted` are the file names of the converted model (glTF, bin and
 * images), which an original must never take. The record is
 * `{ files, meta: { format, source, companions? } }`: `files[0]` is the source,
 * and `companions` maps each companion's stored name to its place relative to
 * the original's folder.
 */
export function planOriginal(
  names: StagedNames,
  format: string,
  source: string,
  companions: string[],
  converted: string[],
): OriginalPlan {
  for (const name of converted) names.converted.add(name.toLowerCase());
  const take = (path: string): string => {
    const known = names.byPath.get(path);
    if (known && !names.converted.has(known.toLowerCase())) return known;
    const name = freeName(baseOf(path), (lower) => names.converted.has(lower) || names.originals.has(lower));
    names.originals.add(name.toLowerCase());
    names.byPath.set(path, name);
    return name;
  };
  const files: StagedFile[] = [{ from: source, name: take(source) }];
  const placements: Record<string, string> = {};
  for (const path of companions) {
    const name = take(path);
    if (files.some((f) => f.name.toLowerCase() === name.toLowerCase())) continue;
    files.push({ from: path, name });
    placements[name] = companionPlacement(source, path);
  }
  return {
    files,
    origin: {
      files: files.map((f) => f.name),
      meta: { format, source, ...(Object.keys(placements).length ? { companions: placements } : {}) },
    },
  };
}
