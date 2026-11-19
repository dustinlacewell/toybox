/**
 * The Substance import, free of any user interface: render each chosen archive
 * (one material, or one per preset) through the engine and stage the maps. The
 * import panel calls `importArchives` with the archives and settings the user
 * chose and the archive it already has open; `tbx import sbsar` calls
 * `importArchivePaths`, which renders with default parameters, a preset and/or
 * parameter values given as text. The caller commits the entries.
 */

import { assertCategory, importNamespace, mergeImportOutcome, newImportOutcome, noteSkip, resolveSources, type AssetView, type FsApi, type ImportOutcome, type MaterialSeedEntry, type ReplacedItem } from "@ldlework/toybox-sdk";

import { absolutePath, fingerprintArchive } from "./archive.js";
import { Substance, type Env } from "./engine.js";
import { changedValues, cleanName, materialId, materialName, presetValues, readMeta, sameValue, sizeOf, type SbsarMeta, type Values } from "./graph.js";
import { shortRendering } from "./describe.js";
import { applyParameters, checkPreset, type RawValue } from "./params.js";
import { produceMaterial } from "./produce.js";

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

export const stemOf = (path: string) => (path.split("/").pop() ?? path).replace(/\.sbsar$/i, "");
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
  graph: number;
  preset: string | null;
  inputs: Values;
  size: number;
  archiveHash?: string;
}

/** Whether the recorded `meta` of a material is the rendering `wanted`: the same archive content, graph, preset,
 *  changed parameters and size. A material recorded without an archive fingerprint never matches. */
export function sameRendering(meta: SbsarMeta | undefined, wanted: Rendering): boolean {
  if (!meta || !meta.archiveHash || !wanted.archiveHash || meta.archiveHash !== wanted.archiveHash) return false;
  if (meta.graph !== wanted.graph || meta.preset !== wanted.preset || meta.size !== wanted.size) return false;
  const keys = new Set([...Object.keys(meta.inputs), ...Object.keys(wanted.inputs)]);
  return [...keys].every((key) => key in meta.inputs && key in wanted.inputs && sameValue(meta.inputs[key], wanted.inputs[key]));
}

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
  /** Finds an item of the library by id. With it, a material that replaces an existing one is reported in `replaced`. */
  lookup?(id: string): Promise<Pick<AssetView, "origin" | "name"> | undefined>;
  /** Skip a material the library already has (found with `lookup`), rendered from the same archive content with the
   *  same graph, preset, parameters and size. */
  skipExisting?: boolean;
  /** The material's name in place of the archive's file name (the base of `<name> - <preset>` with every preset).
   *  Only meaningful for one archive: variants of it then sit side by side. */
  name?: string;
  /** The caller renders every archive's first graph at its defaults: a skip can then be decided without the engine. */
  plain?: boolean;
}

export interface ArchiveImportResult {
  entries: MaterialSeedEntry[];
  /** One line per archive that failed: which one, which step, what the engine or the disk said. */
  problems: string[];
  /** Names of the materials left alone because the library already has them. */
  unchanged: string[];
  /** Materials in `entries` whose id the library already had: committing them replaces those items. */
  replaced: ReplacedItem[];
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
 *  is reported in `problems` and the others go on. Ids follow the source folder,
 *  so importing it again updates its materials; the staged files get a namespace
 *  of their own every time. */
export async function importArchives(request: ArchiveImportRequest): Promise<ArchiveImportResult> {
  const { env, libraryRoot, source, archives, category, size, progress } = request;
  const identity = importNamespace(source);
  const pack = importNamespace(`${source}#${Date.now()}`);
  const result: ArchiveImportResult = { entries: [], problems: [], unchanged: [], replaced: [] };
  const names = new Set<string>();
  const found = async (id: string) => (request.lookup ? await request.lookup(id).catch(() => undefined) : undefined);
  const existing = async (id: string) => (await found(id))?.origin?.meta;
  const named = request.name ? cleanName(request.name) : undefined;
  let index = 0;
  for (const path of archives) {
    const stem = stemOf(path);
    progress?.(`Archive ${++index} of ${archives.length}: ${stem}`);
    let hash: Promise<string | undefined> | undefined;
    const archiveHash = () => (hash ??= fingerprintArchive(env.fs, { root: source, path }).catch(() => undefined));
    try {
      if (request.plain && request.skipExisting) {
        const name = materialName(named ?? stem, null);
        const meta = readMeta(await existing(materialId(identity, name)));
        if (sameRendering(meta, { graph: 0, preset: null, inputs: {}, size: sizeOf(size), archiveHash: await archiveHash() })) {
          names.add(name.toLowerCase());
          result.unchanged.push(name);
          continue;
        }
      }
      const loadedNow = await step("loading in the engine", () => request.open(path));
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
        const id = materialId(identity, name);
        if (request.skipExisting) {
          const wanted: Rendering = { graph: graph.index, preset: variant.preset, inputs: changedValues(values, presetValues(graph, variant.preset)), size: sizeOf(size), archiveHash: await archiveHash() };
          if (sameRendering(readMeta(await existing(id)), wanted)) { result.unchanged.push(name); continue; }
        }
        if (variants.length > 1) progress?.(`Archive ${index} of ${archives.length}: ${name}`);
        const before = await found(id);
        if (before) result.replaced.push({ id, name, was: describeWas(before.origin?.meta) });
        result.entries.push(await produceMaterial(env, loadedNow, libraryRoot, {
          id, pack, category, name, archive: { root: source, path }, preset: variant.preset, values, size: sizeOf(size), archiveHash: await archiveHash(),
        }));
      }
    } catch (failure) { result.problems.push(`${stem}: ${message(failure)}`); }
  }
  return result;
}

/** The entries to commit. An archive that could not be rendered, and a named file that is not an archive, are in `skippedNotes` (without the "skipped" prefix: they start with the file's name). */
export type SbsarImportOutcome = ImportOutcome<MaterialSeedEntry>;

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
  lookup?: (id: string) => Promise<Pick<AssetView, "origin" | "name"> | undefined>,
  progress?: (message: string) => void,
): Promise<SbsarImportOutcome> {
  assertCategory(options.category);
  const total: SbsarImportOutcome = newImportOutcome();
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
  for (const { root, archives } of found) {
    const result = await importArchives({
      env, libraryRoot, source: root, archives, category: options.category, size: options.size, progress,
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
    mergeImportOutcome(total, { entries: result.entries, skipped: result.problems.length, skippedNotes: result.problems, warnings: result.problems, unchanged: result.unchanged, replaced: result.replaced });
  }
  return total;
}
