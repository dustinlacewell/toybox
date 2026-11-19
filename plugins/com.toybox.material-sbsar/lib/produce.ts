/**
 * From a render to a library material: copy the maps a material uses into
 * import staging, put the archive next to them so the material can be rendered
 * again, and build the entry to commit. The archive then lives in the
 * material's own folder in the library.
 */

import type { ImportProvenance, MaterialMapInput, MaterialSeedEntry } from "@ldlework/toybox-sdk";

import { fingerprintArchive, recordArchive, stageArchive, type ArchiveRef } from "./archive.js";
import { releaseRender, type Env, type Rendering, type Substance } from "./engine.js";
import { changedValues, channelOf, outputRoles, presetValues, type SbsarMeta, type Values } from "./graph.js";

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
  /** Where the archive is now: a folder the plugin may read, and the path of
   *  the file under it. For a new import, the picked folder; when re-rendering
   *  a material, the library root and the archive inside that material. */
  archive: ArchiveRef;
  /** The archive's fingerprint when the caller has it (a re-render keeps the recorded one); else it is computed. */
  archiveHash?: string;
  preset: string | null;
  values: Values;
  size: number;
  provenance?: ImportProvenance;
}

/** Run one step of producing a material; a failure says which step. */
export async function inStep<T>(what: string, work: () => Promise<T>): Promise<T> {
  try { return await work(); } catch (failure) { throw new Error(`${what}: ${failure instanceof Error ? failure.message : String(failure)}`); }
}

/** Render `request` and stage it: the maps land in import staging and the
 *  returned entry is ready for `ctx.commit`. */
export async function produceMaterial(env: Env, substance: Substance, libraryRoot: string, request: MaterialRequest): Promise<MaterialSeedEntry> {
  const rendering = await inStep(`rendering ${2 ** request.size} px`, () => substance.render(request.values, request.size));
  const maps: MaterialMapInput[] = [];
  for (const { role, file } of renderedMaps(substance, rendering)) {
    const target = `${role}.png`;
    await inStep(`copying the ${role} map`, () => env.fs.copyFile(rendering.scratch, `${rendering.folder}/${file}`, libraryRoot, `library/${request.pack}/materials/${request.name}/${target}`));
    const channel = channelOf(role);
    maps.push(channel ? { role, file: target, channel } : { role, file: target });
  }
  // The archive goes next to the maps (see archive.ts for where it is kept).
  const sbsar = await inStep("copying the archive", () => stageArchive(env.fs, libraryRoot, request.pack, request.name, request.archive));
  const record = recordArchive(sbsar, request.archiveHash ?? await fingerprintArchive(env.fs, request.archive).catch(() => undefined));
  const meta: SbsarMeta = {
    ...record.meta, graph: substance.graph.index, preset: request.preset,
    inputs: changedValues(request.values, presetValues(substance.graph, request.preset)), size: request.size,
    ...(substance.graph.index > 0 ? { graphLabel: substance.graph.label } : {}),
  };
  await releaseRender(env, rendering);
  return {
    kind: "material", id: request.id, pack: request.pack, category: request.category, name: request.name, maps, flipNormalGreen: false, format: "sbsar",
    origin: { files: record.files, meta: { ...meta } },
    ...(request.provenance ? { provenance: request.provenance } : {}),
  };
}
