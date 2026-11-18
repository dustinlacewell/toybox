/**
 * Source-tree discovery and per-asset materialization. Pure orchestration over
 * the host's jailed fs: walk a picked source root, find every model file a
 * format plugin handles, and hand each to that format. Kept free of React so the
 * import logic is testable and reads top-down. This plugin converts nothing
 * itself: with no format turned on it finds nothing.
 */

import type { FsApi, SeedOrigin } from "@ldlework/toybox-sdk";
import { importWithFormat, modelExtension, type FormatTable } from "./formats.js";
import { cleanCompanions, planOriginal, type StagedNames } from "./originals.js";

/** A model file discovered under the source root. */
export interface Found {
  /** Source-root-relative path (forward-slashed), as returned by readDir. */
  relPath: string;
  /** Basename without extension — the asset name and target stem. */
  stem: string;
  /** Lower-case extension of a format a plugin handles. */
  ext: string;
}

/** Recursively list every model file under `sourceRoot` whose extension is in `table`. */
export async function findModels(fs: FsApi, sourceRoot: string, table: FormatTable): Promise<Found[]> {
  const out: Found[] = [];
  if (!table.extensions.length) return out;
  const queue: string[] = ["."];
  while (queue.length) {
    const dir = queue.shift()!;
    for (const e of await fs.readDir(sourceRoot, dir)) {
      if (e.isDir) {
        queue.push(e.relPath);
        continue;
      }
      const ext = modelExtension(e.name, table);
      if (ext) out.push({ relPath: e.relPath, stem: stemOf(e.name), ext });
    }
  }
  return out;
}

/** The outcome of materializing one found model into the library. */
export interface Materialized {
  /** Library-relative `.gltf` filename the seed entry references. */
  file: string;
  /** Human-facing note for the report. */
  warnings: string[];
  /** The companion files of the source the format returned (paths relative to
   *  the source root), and the plugin that contributed the format. */
  originals?: string[];
  pluginId?: string;
  /** False for a format whose result already is the stored file. */
  keepsOriginal?: boolean;
}

/** A rejection reason: a free-form message from a format plugin. */
export type Reject = string;

/** The host capabilities `materialize` needs: the jailed fs, the picked source
 *  root, the library root and the formats other plugins contributed. */
export interface Imports {
  fs: FsApi;
  sourceRoot: string;
  libraryRoot: string;
  formats: FormatTable;
}

/**
 * Land one found model into `library/<pack>/<category>/` by handing it to the
 * format plugin that claimed its extension. The format must produce the library
 * invariant the index assumes: a loose glTF whose single buffer is `<stem>.bin`,
 * beside its `.bin` and textures.
 */
export async function materialize(
  io: Imports,
  found: Found,
  pack: string,
  category: string,
): Promise<Materialized | { rejected: Reject }> {
  return importWithFormat(
    io.formats,
    found.ext,
    { root: io.sourceRoot, path: found.relPath, stem: found.stem },
    { libraryRoot: io.libraryRoot, pack, category },
  );
}

/**
 * Record where a model came from. A format that keeps the original gets the
 * source file and the companions it named copied beside the converted glTF in
 * the staging folder, and the `origin` to commit with the entry. A format that
 * does not (glTF: the stored file is the original) gets an origin that names only
 * its plugin, so turning that plugin off hides what it imported. A file that
 * cannot be copied is left out with a warning; when the source itself cannot be
 * copied the entry has no origin. The glTF the format wrote is read to keep
 * originals off the names of its images.
 */
export async function keepOriginal(
  io: Imports,
  found: Found,
  imported: Materialized,
  pack: string,
  category: string,
  names: StagedNames,
): Promise<{ origin?: SeedOrigin; warnings: string[] }> {
  const warnings: string[] = [];
  if (!imported.pluginId) return { warnings };
  if (imported.keepsOriginal === false) return { origin: { plugin: imported.pluginId }, warnings };
  const dest = `library/${pack}/${category}`;
  const converted = [imported.file, `${found.stem}.bin`];
  try {
    const doc = JSON.parse(new TextDecoder().decode(await io.fs.readBytes(io.libraryRoot, `${dest}/${imported.file}`))) as GltfDoc;
    for (const uri of imageUris(doc)) if (!uri.startsWith("data:")) converted.push(uri.slice(uri.lastIndexOf("/") + 1));
  } catch { /* the names of the glTF and bin are enough to protect */ }

  const plan = planOriginal(names, found.ext, found.relPath, cleanCompanions(imported.originals, found.relPath), converted);
  const kept: typeof plan.files = [];
  for (const file of plan.files) {
    try {
      await io.fs.copyFile(io.sourceRoot, file.from, io.libraryRoot, `${dest}/${file.name}`);
      kept.push(file);
    } catch (e) {
      if (file === plan.files[0]) {
        warnings.push(`The original of ${found.relPath} could not be kept: ${e instanceof Error ? e.message : String(e)}`);
        return { warnings };
      }
      warnings.push(`${file.from} belongs to ${found.relPath} but could not be kept.`);
    }
  }
  const files = kept.map((f) => f.name);
  const meta = { ...(plan.origin.meta ?? {}) } as Record<string, unknown>;
  if (meta.companions) {
    const left = Object.fromEntries(Object.entries(meta.companions as Record<string, string>).filter(([name]) => files.includes(name)));
    if (Object.keys(left).length) meta.companions = left; else delete meta.companions;
  }
  return { origin: { plugin: imported.pluginId, files, meta }, warnings };
}

// --- pure helpers ------------------------------------------------------------

function stemOf(name: string): string {
  return name.replace(/\.[^.]+$/, "");
}

function imageUris(doc: GltfDoc): string[] {
  return (doc.images ?? []).map((i) => i.uri).filter((u): u is string => !!u);
}

interface GltfDoc {
  images?: { uri?: string }[];
}
