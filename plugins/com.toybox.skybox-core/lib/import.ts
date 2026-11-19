/**
 * The sky import, free of any user interface: find the skies in a folder of
 * images, read each one's size and layout from its file header, copy its files
 * into import staging and build the seed entries. The import panel calls
 * `importSkies` with the skies the user ticked and the layouts they chose;
 * `tbx import skybox` calls `importSkyPaths`. The caller commits.
 */

import { assertCategory, importNamespace, mergeImportOutcome, newImportOutcome, noteSkip, resolveSources, type FsApi, type PluginSeedEntry } from "@ldlework/toybox-sdk";

import { FACES, cleanName, findSkies, isSkyImage, layoutFromSize, skyId, type ImageLayout, type SkyCandidate, type SkyMeta } from "./detect.js";
import { imageSize } from "./header.js";

export type LayoutChoice = "auto" | ImageLayout;
/** The layouts one image can be told to have. */
export const IMAGE_LAYOUTS: ImageLayout[] = ["equirect", "cross-horizontal", "cross-vertical", "strip-horizontal", "strip-vertical"];

export const fileName = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/** Every sky image under `source`, source-relative and forward-slashed. */
export async function findImages(fs: Pick<FsApi, "readDir">, source: string): Promise<string[]> {
  const files: string[] = [];
  const seen = new Set<string>();
  async function walk(dir: string) {
    if (seen.has(dir)) return;
    seen.add(dir);
    for (const entry of await fs.readDir(source, dir)) {
      if (entry.isDir) await walk(entry.relPath);
      else if (isSkyImage(entry.name)) files.push(entry.relPath.replace(/\\/g, "/"));
    }
  }
  await walk(".");
  return files;
}

export interface Measured { meta: SkyMeta; certain: boolean; }

/** A sky's metadata: the layout chosen or read from the image's shape. `read`
 *  returns a source-relative file's bytes. */
export async function measureSky(read: (path: string) => Promise<Uint8Array>, sky: SkyCandidate, choice: LayoutChoice): Promise<Measured> {
  const size = imageSize(await read(sky.files[0]));
  if (!size) throw new Error(`${fileName(sky.files[0])} is not a readable ${sky.format} image`);
  const base = { encoding: sky.encoding, format: sky.format, width: size.width, height: size.height };
  if (sky.layout === "cubemap") {
    if (size.width !== size.height) throw new Error(`Cube faces must be square; ${fileName(sky.files[0])} is ${size.width} × ${size.height}`);
    const faces = Object.fromEntries(FACES.map((face) => [face, fileName(sky.faces![face])])) as SkyMeta["faces"];
    return { meta: { ...base, layout: "cubemap", faces }, certain: true };
  }
  const guess = layoutFromSize(size.width, size.height);
  return choice === "auto" ? { meta: { ...base, layout: guess.layout }, certain: guess.certain } : { meta: { ...base, layout: choice }, certain: true };
}

export interface SkyImportOutcome {
  entries: PluginSeedEntry[];
  /** How many skies were left out; `skippedNotes` says why (they are also in `warnings`). */
  skipped: number;
  skippedNotes: string[];
  warnings: string[];
}

export interface SkyImportInput {
  /** The folder the skies' files are read under. */
  source: string;
  skies: SkyCandidate[];
  /** The layout chosen per sky name; absent means automatic. */
  layouts?: ReadonlyMap<string, LayoutChoice>;
  category: string;
  /** Staging namespace, from `importNamespace`. */
  pack: string;
  /** Leave out a sky that cannot be read (and say why) instead of failing the whole import. */
  lenient?: boolean;
  /** Reads a source-relative file; defaults to the jailed `fs`. A panel passes the reader that caches the bytes it already measured. */
  read?: (path: string) => Promise<Uint8Array>;
}

/** Measure each sky, copy its files (natively: they are too large to pass
 *  through here) to `library/<pack>/skybox/<name>/` under `libraryRoot` and
 *  build its entry. */
export async function importSkies(
  io: { fs: Pick<FsApi, "readBytes" | "copyFile">; libraryRoot: string },
  input: SkyImportInput,
  progress?: (message: string) => void,
): Promise<SkyImportOutcome> {
  const { source, skies, pack, category } = input;
  const read = input.read ?? ((path: string) => io.fs.readBytes(source, path));
  const outcome: SkyImportOutcome = { entries: [], skipped: 0, skippedNotes: [], warnings: [] };
  let index = 0;
  for (const sky of skies) {
    progress?.(`Copying ${++index} of ${skies.length}: ${sky.name}`);
    try {
      const name = cleanName(sky.name);
      let meta: SkyMeta;
      try { meta = (await measureSky(read, sky, input.layouts?.get(sky.name) ?? "auto")).meta; }
      catch (failure) { throw new Error(`${sky.name}: ${failure instanceof Error ? failure.message : String(failure)}`); }
      const files = sky.files.map(fileName);
      for (const path of sky.files) await io.fs.copyFile(source, path, io.libraryRoot, `library/${pack}/skybox/${name}/${fileName(path)}`);
      outcome.entries.push({ kind: "plugin", type: "skybox", id: skyId(pack, name), pack, category, name, files, format: files[0]?.split(".").pop()?.toLowerCase(), meta: { ...meta } });
    } catch (failure) {
      if (!input.lenient) throw failure;
      const note = `skipped ${sky.name}: ${(failure instanceof Error ? failure.message : String(failure)).replace(`${sky.name}: `, "")}`;
      outcome.skipped += 1;
      outcome.skippedNotes.push(note);
      outcome.warnings.push(note);
    }
  }
  return outcome;
}

/** The whole import for paths given as folders and image files. In a folder the
 *  skies the panel ticks by default are imported (HDR and EXR images, cube maps
 *  and images whose name says they are a sky), or every image with `all`; images
 *  named one by one are all imported. `layout` applies to single-image skies. */
export async function importSkyPaths(
  io: { fs: FsApi; libraryRoot: string },
  paths: string[],
  options: { category: string; layout: LayoutChoice; all?: boolean },
  progress?: (message: string) => void,
): Promise<SkyImportOutcome> {
  assertCategory(options.category);
  const total: SkyImportOutcome = newImportOutcome();
  const note = (text: string) => noteSkip(total, text);
  for (const source of await resolveSources(io.fs, paths)) {
    let images: string[];
    if (source.files) {
      images = source.files.filter(isSkyImage);
      for (const file of source.files.filter((name) => !isSkyImage(name))) note(`skipped ${file}: not an HDR, EXR, PNG, JPG or WebP image`);
    } else {
      images = await findImages(io.fs, source.root);
    }
    const found = findSkies(images);
    const chosen = source.files || options.all ? found : found.filter((sky) => sky.likely);
    if (found.length > chosen.length && !chosen.length) {
      note(`skipped ${found.length} image${found.length === 1 ? "" : "s"} in ${source.root}: none look like skies (use --all to import them anyway)`);
    }
    const layouts = new Map(chosen.map((sky) => [sky.name, options.layout] as const));
    const outcome = await importSkies(io, { source: source.root, skies: chosen, layouts, category: options.category, pack: importNamespace(source.root), lenient: true }, progress);
    mergeImportOutcome(total, outcome);
  }
  return total;
}
