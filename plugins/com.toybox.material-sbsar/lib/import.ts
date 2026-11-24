/**
 * The Substance import, free of any user interface. Each archive becomes a
 * substance (see `substance.ts`; added once per library, found again by the
 * archive's content) and one or more materials rendered from it through the
 * engine (one material, or one per preset). The import panel calls
 * `importArchives` with the archives and settings the user chose and the
 * archive it already has open; `tbx import sbsar` calls `importArchivePaths`,
 * which renders with default parameters, a preset and/or parameter values given
 * as text. The caller commits the entries: a new substance comes before the
 * materials made from it.
 */

import { assertCategory, importNamespace, mergeImportOutcome, newImportOutcome, noteSkip, resolveSources, type AssetView, type FsApi, type ImportOutcome, type MaterialSeedEntry, type PluginSeedEntry, type ReplacedItem } from "@ldlework/toybox-sdk";

import { Substance, type Env } from "./engine.js";
import { changedValues, cleanName, materialId, materialName, presetValues, readMeta, sameValue, sizeOf, type SbsarMeta, type Values } from "./graph.js";
import { shortRendering } from "./describe.js";
import { applyParameters, checkPreset, type RawValue } from "./params.js";
import { produceMaterial, stagedMap, writePreview } from "./produce.js";
import { absolutePath, baseName, isSubstance, sameId, stageSubstanceArchive, substanceEntry, substanceId, substanceStaging } from "./substance.js";

/** What to render an archive with. */
export interface ArchiveConfig {
  preset: string | null;
  values: Values;
  /** Import every preset of the archive as a material of its own. */
  everyPreset: boolean;
  /** Parameter values given as text or JSON, laid over each variant's preset values and checked against the archive. */
  given?: Record<string, RawValue>;
  /** A given parameter the archive lacks is left out (with a warning) instead of failing that archive. */
  ignoreMissing?: boolean;
}

export const stemOf = (path: string) => baseName(path).replace(/\.sbsar$/i, "");
const message = (failure: unknown) => (failure instanceof Error ? failure.message : String(failure));

/** Every `.sbsar` under `source`, source-relative, forward-slashed and sorted. */
export async function findArchives(fs: Pick<FsApi, "readDir">, source: string): Promise<string[]> {
  const files: string[] = [];
  const queue = ["."];
  const seen = new Set<string>();
  while (queue.length) {
    const dir = queue.shift()!;
    if (seen.has(dir)) continue;
    seen.add(dir);
    for (const entry of await fs.readDir(source, dir)) {
      if (entry.isDir) queue.push(entry.relPath);
      else if (/\.sbsar$/i.test(entry.name)) files.push(entry.relPath.replace(/\\/g, "/"));
    }
  }
  return files.sort((a, b) => a.localeCompare(b));
}

/** What a material was (or would be) rendered from, to tell whether it is already in the library. */
export interface Rendering {
  /** The substance the material is made from. */
  substance: string;
  graph: number;
  preset: string | null;
  inputs: Values;
  size: number;
}

/** Whether the stored material `existing` is the rendering `wanted`: made from the same substance (so the same
 *  archive content) with the same graph, preset, changed parameters and size. A material that names no substance
 *  (one from before substances, in a library not converted yet) never matches. */
export function sameRendering(existing: Pick<AssetView, "origin"> | undefined, wanted: Rendering): boolean {
  if (!existing || !sameId(existing.origin?.derivedFrom, wanted.substance)) return false;
  const meta: SbsarMeta | undefined = readMeta(existing.origin?.meta);
  if (!meta || meta.graph !== wanted.graph || meta.preset !== wanted.preset || meta.size !== wanted.size) return false;
  const keys = new Set([...Object.keys(meta.inputs), ...Object.keys(wanted.inputs)]);
  return [...keys].every((key) => key in meta.inputs && key in wanted.inputs && sameValue(meta.inputs[key], wanted.inputs[key]));
}

type Found = Pick<AssetView, "origin" | "name" | "kind"> & { type?: string };

export interface ArchiveImportRequest {
  env: Env;
  libraryRoot: string;
  /** The folder the archives are under. */
  source: string;
  /** Source-relative paths of the archives to render. */
  archives: string[];
  category: string;
  /** The texture size as the engine counts it (a power of two), e.g. 11 for 2048. */
  size: number;
  /** The archive loaded into the engine, reusing one the caller already has open. */
  open(path: string): Promise<Substance>;
  /** How to render `path` once loaded; undefined means its defaults. */
  configOf(path: string, substance: Substance): ArchiveConfig | undefined;
  progress?(message: string): void;
  warn?(message: string): void;
  /** Finds an item of the library by its raw id. With it an archive whose substance is in the library already is
   *  not staged again, and a material that replaces an existing one is reported in `replaced`. Without it every
   *  archive's substance is staged (committing it again replaces it with itself). */
  lookup?(id: string): Promise<Found | undefined>;
  /** Skip a material the library already has (found with `lookup`), rendered from the same substance with the
   *  same graph, preset, parameters and size. */
  skipExisting?: boolean;
  /** The material's name in place of the archive's file name (the base of `<name> - <preset>` with every preset),
   *  and the name of a substance this import adds. Only meaningful for one archive. */
  name?: string;
  /** The caller renders every archive's first graph at its defaults: a skip can then be decided without the engine. */
  plain?: boolean;
  /** Substance ids already staged by this import (shared by the calls of one import over several folders), so
   *  the same archive found twice is stored once. */
  staged?: Set<string>;
}

export interface ArchiveImportResult {
  /** New substances, each before the first material made from it, and the materials. */
  entries: (MaterialSeedEntry | PluginSeedEntry)[];
  /** One line per archive that failed: which one, which step, what the engine or the disk said. */
  problems: string[];
  /** Names of the materials left alone because the library already has them. */
  unchanged: string[];
  /** Materials in `entries` whose id the library already had: committing them replaces those items. */
  replaced: ReplacedItem[];
  /** Names of the substances in `entries` (archives new to the library). */
  substances: string[];
}

/** How an existing material was rendered, in a few words (what an import that replaces it says). */
function describeWas(meta: unknown): string | undefined {
  const parsed = readMeta(meta);
  return parsed ? shortRendering(parsed) : undefined;
}

/** Run one step of an archive's import; a failure says which step. */
async function step<T>(what: string, work: () => Promise<T>): Promise<T> {
  try { return await work(); } catch (failure) { throw new Error(`${what}: ${message(failure)}`); }
}

/** Render every archive and build the entries to commit. An archive that fails
 *  is reported in `problems` and the others go on. Material ids follow the
 *  source folder, so importing it again updates its materials; the staged files
 *  get a namespace of their own every time. */
export async function importArchives(request: ArchiveImportRequest): Promise<ArchiveImportResult> {
  const { env, libraryRoot, source, archives, category, size, progress } = request;
  const identity = importNamespace(source);
  const pack = importNamespace(`${source}#${Date.now()}`);
  const result: ArchiveImportResult = { entries: [], problems: [], unchanged: [], replaced: [], substances: [] };
  const names = new Set<string>();
  const staged = request.staged ?? new Set<string>();
  const found = async (id: string) => (request.lookup ? await request.lookup(id).catch(() => undefined) : undefined);
  const named = request.name ? cleanName(request.name) : undefined;
  let index = 0;
  for (const path of archives) {
    const stem = stemOf(path);
    progress?.(`Archive ${++index} of ${archives.length}: ${stem}`);
    const first = result.entries.length;
    let substance: { id: string; sha256: string; isNew: boolean; pack: string; name: string } | undefined;
    let loaded: Substance | undefined;
    try {
      const sha256 = await step("reading the archive", () => env.fs.hashFile(source, path));
      const id = substanceId(sha256);
      const existing = await found(id);
      // Already in the library (an item of another type with that id would be a bug; it is not replaced).
      const known = staged.has(id) || (!!existing && isSubstance(existing));
      if (existing && !isSubstance(existing)) throw new Error(`the library has an item ${id} that is not a substance`);
      substance = { id, sha256, isNew: !known, pack: importNamespace(`${id}#${Date.now()}`), name: cleanName(named ?? stem) };
      if (request.plain && request.skipExisting && known) {
        const name = materialName(named ?? stem, null);
        if (sameRendering(await found(materialId(identity, name)), { substance: id, graph: 0, preset: null, inputs: {}, size: sizeOf(size) })) {
          names.add(name.toLowerCase());
          result.unchanged.push(name);
          continue;
        }
      }
      const loadedNow = loaded = await step("loading in the engine", () => request.open(path));
      const own = request.configOf(path, loadedNow) ?? { preset: null, values: presetValues(loadedNow.graph, null), everyPreset: false };
      const graph = loadedNow.graph;
      const base = named ?? (graph.index > 0 ? `${stem} - ${graph.label}` : stem);
      const variants = own.everyPreset && graph.presets.length
        ? graph.presets.map((preset) => ({ preset: preset.label as string | null, values: presetValues(graph, preset.label), named: true }))
        : [{ preset: own.preset, values: own.values, named: false }];
      for (const variant of variants) {
        let values = variant.values;
        if (own.given && Object.keys(own.given).length) {
          const applied = applyParameters(graph, values, own.given, { ignoreMissing: own.ignoreMissing });
          if (applied.problems.length) throw new Error(applied.problems.map((problem) => problem.message).join("; "));
          for (const name of applied.ignored) request.warn?.(`${stem}: has no parameter "${name}"; ignored`);
          values = applied.values;
        }
        let name = materialName(base, variant.named ? variant.preset : null);
        for (let copy = 2; names.has(name.toLowerCase()); copy++) name = cleanName(`${materialName(base, variant.named ? variant.preset : null)} ${copy}`);
        names.add(name.toLowerCase());
        const materialIdNow = materialId(identity, name);
        if (request.skipExisting && known) {
          const wanted: Rendering = { substance: id, graph: graph.index, preset: variant.preset, inputs: changedValues(values, presetValues(graph, variant.preset)), size: sizeOf(size) };
          if (sameRendering(await found(materialIdNow), wanted)) { result.unchanged.push(name); continue; }
        }
        if (variants.length > 1) progress?.(`Archive ${index} of ${archives.length}: ${name}`);
        const before = await found(materialIdNow);
        if (before) result.replaced.push({ id: materialIdNow, name, was: describeWas(before.origin?.meta) });
        const produced = await produceMaterial(env, loadedNow, libraryRoot, {
          id: materialIdNow, pack, category, name, derivedFrom: id, sbsar: baseName(path), preset: variant.preset, values, size: sizeOf(size),
        });
        result.entries.push(produced.entry);
      }
    } catch (failure) { result.problems.push(`${stem}: ${message(failure)}`); }
    // A new substance is staged only once something was made from it (so an archive that fails stages nothing of
    // its own), and goes before those materials. Its preview picture is the first material's staged colour map.
    if (substance?.isNew && result.entries.length > first) {
      try {
        const sbsar = await step("copying the archive", () => stageSubstanceArchive(env.fs, libraryRoot, substance!.pack, substance!.name, { root: source, path }));
        const bytes = await env.fs.fileSize(source, path).catch(() => undefined);
        const albedo = stagedMap(result.entries[first] as MaterialSeedEntry, "albedo");
        const preview = !!albedo && await writePreview(env.fs, { root: libraryRoot, path: albedo }, sizeOf(size), libraryRoot, substanceStaging(substance.pack, substance.name));
        result.entries.splice(first, 0, substanceEntry({
          id: substance.id, pack: substance.pack, category, name: substance.name, sbsar, sha256: substance.sha256, bytes,
          graphs: loaded?.graphs, preview,
        }));
        staged.add(substance.id);
        result.substances.push(substance.name);
      } catch (failure) {
        // Its materials would name a substance that is not there: leave them out too, and tidy their staged files
        // where the host can remove files (only `tbx` can; in the app they wait in staging, never committed).
        const dropped = result.entries.splice(first);
        for (const entry of dropped) await env.fs.removeTree?.(libraryRoot, `library/${entry.pack}/materials/${entry.name}`).catch(() => undefined);
        await env.fs.removeTree?.(libraryRoot, substanceStaging(substance.pack, substance.name)).catch(() => undefined);
        result.replaced = result.replaced.filter((item) => !dropped.some((entry) => entry.id === item.id));
        result.problems.push(`${stem}: ${message(failure)}`);
      }
    }
  }
  return result;
}

/** The entries to commit. An archive that could not be rendered, and a named file that is not an archive, are in `skippedNotes` (without the "skipped" prefix: they start with the file's name). */
export type SbsarImportOutcome = ImportOutcome<MaterialSeedEntry | PluginSeedEntry> & { substances: string[] };

export interface ArchivePathOptions {
  category: string;
  /** The texture size as the engine counts it (a power of two). */
  size: number;
  preset?: string;
  everyPreset?: boolean;
  /** The archive's graph, by index or label; the first when absent. */
  graph?: number | string;
  /** `name → value` texts or JSON values (`--set`, `--values`). */
  given?: Record<string, RawValue>;
  ignoreMissing?: boolean;
  skipExisting?: boolean;
  /** The material's name instead of the archive's file name; for one archive only. */
  name?: string;
}

/** The whole import for paths given as folders and `.sbsar` files. Each archive
 *  is rendered at `size` with its default parameters, or with the preset named
 *  by `preset`, or once per preset with `everyPreset`, with `given` laid over. */
export async function importArchivePaths(
  env: Env,
  libraryRoot: string,
  paths: string[],
  options: ArchivePathOptions,
  lookup?: (id: string) => Promise<Found | undefined>,
  progress?: (message: string) => void,
): Promise<SbsarImportOutcome> {
  assertCategory(options.category);
  const total: SbsarImportOutcome = { ...newImportOutcome<MaterialSeedEntry | PluginSeedEntry>(), substances: [] };
  const given = options.given ?? {};
  const customised = !!options.preset || !!options.everyPreset || Object.keys(given).length > 0;
  // Find every archive first, so a mistake that concerns the whole command is reported before anything renders.
  const found: { root: string; archives: string[] }[] = [];
  for (const source of await resolveSources(env.fs, paths)) {
    let archives: string[];
    if (source.files) {
      archives = source.files.filter((file) => /\.sbsar$/i.test(file));
      for (const file of source.files.filter((name) => !/\.sbsar$/i.test(name))) noteSkip(total, `${file}: not a Substance archive (.sbsar)`);
    } else {
      archives = await findArchives(env.fs, source.root);
    }
    if (archives.length) found.push({ root: source.root, archives });
  }
  const count = found.reduce((sum, item) => sum + item.archives.length, 0);
  if (options.name !== undefined) {
    if (!options.name.trim() || !cleanName(options.name).trim()) throw new Error("--name needs a name for the material.");
    if (count > 1) throw new Error(`--name gives one material its name, but ${count} archives are selected. Import them one at a time, or leave --name out.`);
  }
  const staged = new Set<string>();
  for (const { root, archives } of found) {
    const result = await importArchives({
      env, libraryRoot, source: root, archives, category: options.category, size: options.size, progress, staged,
      warn: (text) => total.warnings.push(text),
      lookup,
      skipExisting: options.skipExisting && !!lookup,
      name: options.name,
      plain: !customised && (options.graph === undefined || String(options.graph) === "0"),
      open: (path) => Substance.open(env, absolutePath({ root, path }), options.graph ?? 0),
      configOf: (_path, substance) => {
        if (options.preset) checkPreset(substance.graph, options.preset);
        if (!customised) return undefined;
        const preset = options.everyPreset ? null : options.preset ?? null;
        return { preset, values: presetValues(substance.graph, preset), everyPreset: !!options.everyPreset, given, ignoreMissing: options.ignoreMissing };
      },
    });
    // Skip notes are listed among the warnings too, as `ImportOutcome` asks; `runImport` reports each once.
    mergeImportOutcome(total, { entries: result.entries, skipped: result.problems.length, skippedNotes: result.problems, warnings: result.problems, unchanged: result.unchanged, replaced: result.replaced });
    total.substances.push(...result.substances);
  }
  return total;
}
