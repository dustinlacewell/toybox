/**
 * From a render to a library material: copy the maps a material uses into
 * import staging and build the entry to commit. The archive is not part of the
 * material: it belongs to the material's substance, named by
 * `origin.derivedFrom` (see `substance.ts`). A staged colour map can also give
 * a new substance its `preview.png` (`writePreview`).
 */

import type { FsApi, ImportProvenance, MaterialMapInput, MaterialSeedEntry } from "@ldlework/toybox-sdk";

import { releaseRender, type Env, type Rendering, type Substance } from "./engine.js";
import type { ArchiveRef } from "./substance.js";
import { changedValues, channelOf, outputRoles, presetValues, type SbsarMeta, type Values } from "./graph.js";
import { PREVIEW_FILE, PREVIEW_PIXELS, PREVIEW_SIZE } from "./substance.js";

/** The maps of a material in `rendering`, as `{ role, file }` of the rendered
 *  files. Fails when the archive produced no colour map. */
export function renderedMaps(substance: Substance, rendering: Rendering): { role: MaterialMapInput["role"]; file: string }[] {
  const maps = outputRoles(substance.graph).flatMap(({ identifier, role }) => {
    const file = rendering.files.find((item) => item.identifier === identifier)?.file;
    return file ? [{ role, file }] : [];
  });
  if (!maps.some((map) => map.role === "albedo")) throw new Error("This archive has no base colour output, so it cannot become a material.");
  return maps;
}

export interface MaterialRequest {
  /** Entry id; reuse an existing material's id to replace it. */
  id: string;
  /** Staging namespace for the files, from `importNamespace`. */
  pack: string;
  category: string;
  name: string;
  /** The id of the substance the material is rendered from (raw or qualified). */
  derivedFrom: string;
  /** File name of the substance's archive, recorded in `meta.sbsar`. */
  sbsar: string;
  preset: string | null;
  values: Values;
  size: number;
  provenance?: ImportProvenance;
}

/** Run one step of producing a material; a failure says which step. */
export async function inStep<T>(what: string, work: () => Promise<T>): Promise<T> {
  try { return await work(); } catch (failure) { throw new Error(`${what}: ${failure instanceof Error ? failure.message : String(failure)}`); }
}

/** Where a staged material's map of `role` is, under the library root (until the entry is committed). */
export function stagedMap(entry: Pick<MaterialSeedEntry, "pack" | "name" | "maps">, role: MaterialMapInput["role"]): string | undefined {
  const map = entry.maps.find((item) => item.role === role);
  return map ? `library/${entry.pack}/materials/${entry.name}/${map.file}` : undefined;
}

/** Write `preview.png` (at most 512 px) into the library-relative `folder` from a colour map rendered at `size`
 *  (as the engine counts it). One at 512 px or less is copied as it is; a larger one is scaled down by the host
 *  (`readImageThumb`: PNG bytes fitting the size, never enlarged). Returns false when that fails, e.g. a host that
 *  cannot scale images (the substance then has no preview and is drawn with a placeholder). */
export async function writePreview(fs: Pick<FsApi, "copyFile" | "readImageThumb" | "writeBytes">, from: ArchiveRef, size: number, libraryRoot: string, folder: string): Promise<boolean> {
  const target = `${folder}/${PREVIEW_FILE}`;
  try {
    if (size <= PREVIEW_SIZE) await fs.copyFile(from.root, from.path, libraryRoot, target);
    else await fs.writeBytes(libraryRoot, target, await fs.readImageThumb(from.root, from.path, PREVIEW_PIXELS));
    return true;
  } catch {
    return false;
  }
}

/** Render `request` and stage it: the maps land in import staging and the
 *  returned entry is ready for `ctx.commit`. The render's folder is released either way. */
export async function produceMaterial(env: Env, substance: Substance, libraryRoot: string, request: MaterialRequest): Promise<{ entry: MaterialSeedEntry }> {
  const rendering = await inStep(`rendering ${2 ** request.size} px`, () => substance.render(request.values, request.size));
  try {
    const maps: MaterialMapInput[] = [];
    for (const { role, file } of renderedMaps(substance, rendering)) {
      const target = `${role}.png`;
      await inStep(`copying the ${role} map`, () => env.fs.copyFile(rendering.scratch, `${rendering.folder}/${file}`, libraryRoot, `library/${request.pack}/materials/${request.name}/${target}`));
      const channel = channelOf(role);
      maps.push(channel ? { role, file: target, channel } : { role, file: target });
    }
    const meta: SbsarMeta = {
      sbsar: request.sbsar, graph: substance.graph.index, preset: request.preset,
      inputs: changedValues(request.values, presetValues(substance.graph, request.preset)), size: request.size,
      ...(substance.graph.index > 0 ? { graphLabel: substance.graph.label } : {}),
    };
    return {
      entry: {
        kind: "material", id: request.id, pack: request.pack, category: request.category, name: request.name, maps, flipNormalGreen: false, format: "sbsar",
        origin: { derivedFrom: request.derivedFrom, meta: { ...meta } },
        ...(request.provenance ? { provenance: request.provenance } : {}),
      },
    };
  } finally {
    await releaseRender(env, rendering);
  }
}
