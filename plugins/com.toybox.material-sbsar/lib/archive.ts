/**
 * Where a material's Substance archive is: the only module that stages the
 * archive into the library, records it on an entry, or finds it again.
 *
 * An archive is stored once per library, however many presets are rendered from it:
 *
 * - **staged** by `stageArchive`: copied to `library/<pack>/materials/<name>/<file>`
 *   next to the maps, as an importer always stages;
 * - **recorded** by `recordArchive`: the entry's `origin.files` lists the bare file
 *   name and `origin.meta.sbsar` repeats it. At commit the host moves the staged file
 *   into shared storage (`library/materials/_sources/<sha256>/<file>`, written once; a
 *   copy of an archive that is stored already is dropped) and rewrites `origin.files`
 *   to that library-relative path;
 * - **found** by `locateArchive`: the library-relative path in `asset.origin.files`
 *   whose last segment is `meta.sbsar`, read under the library root. The path is the
 *   host's: nothing here assumes the file sits in the material's folder.
 */

import type { AssetView, FsApi } from "@ldlework/toybox-sdk";

import type { SbsarMeta } from "./graph.js";

const baseName = (path: string) => path.replace(/\\/g, "/").split("/").pop() ?? path;

/** A file the plugin can read: a path under a folder it may read. */
export interface ArchiveRef {
  root: string;
  path: string;
}

/** The absolute path of a file under a root, with forward slashes (what the engine is told). */
export function absolutePath(ref: ArchiveRef): string {
  const root = ref.root.replace(/\\/g, "/");
  return `${root}${root.endsWith("/") ? "" : "/"}${ref.path.replace(/\\/g, "/")}`;
}

/** Copy the archive next to the maps of the material being staged; returns its bare file name. */
export async function stageArchive(fs: Pick<FsApi, "copyFile">, libraryRoot: string, pack: string, name: string, from: ArchiveRef): Promise<string> {
  const file = baseName(from.path);
  await fs.copyFile(from.root, from.path, libraryRoot, `library/${pack}/materials/${name}/${file}`);
  return file;
}

/** What an entry says about its archive: the `origin` fields (`files`) and the `meta` fields that name it. */
export function recordArchive(file: string, hash?: string): { files: string[]; meta: Pick<SbsarMeta, "sbsar"> & { archiveHash?: string } } {
  return { files: [file], meta: { sbsar: file, ...(hash ? { archiveHash: hash } : {}) } };
}

/** The library-relative path of a stored material's archive, or undefined when the item does not list it. */
export function archiveFile(asset: Pick<AssetView, "origin">, meta: Pick<SbsarMeta, "sbsar">): string | undefined {
  return (asset.origin?.files ?? []).find((item) => baseName(item) === meta.sbsar);
}

/** Where the archive of a stored material is, or undefined when the item does not list it. */
export function locateArchive(asset: Pick<AssetView, "origin">, meta: Pick<SbsarMeta, "sbsar">, libraryRoot: string): ArchiveRef | undefined {
  const path = archiveFile(asset, meta);
  return path ? { root: libraryRoot, path } : undefined;
}

/**
 * A fingerprint of a file's content: its length and two 32-bit hashes of its words. It tells
 * whether two files are the same one, quickly; it is not a cryptographic hash.
 */
export function fingerprint(bytes: Uint8Array): string {
  let a = 0x811c9dc5, b = 0x9e3779b1;
  const words = bytes.byteLength >> 2;
  let view: Uint32Array | undefined;
  if (bytes.byteOffset % 4 === 0) view = new Uint32Array(bytes.buffer, bytes.byteOffset, words);
  for (let i = 0; i < words; i++) {
    const word = view ? view[i] : bytes[i * 4] | (bytes[i * 4 + 1] << 8) | (bytes[i * 4 + 2] << 16) | (bytes[i * 4 + 3] << 24);
    a = Math.imul(a ^ word, 0x01000193);
    b = Math.imul(b + word, 0x85ebca6b) ^ (b >>> 15);
  }
  for (let i = words * 4; i < bytes.byteLength; i++) a = Math.imul(a ^ bytes[i], 0x01000193);
  return `${bytes.byteLength.toString(16)}-${(a >>> 0).toString(16).padStart(8, "0")}${(b >>> 0).toString(16).padStart(8, "0")}`;
}

/** The fingerprint of the archive at `ref`. */
export async function fingerprintArchive(fs: Pick<FsApi, "readBytes">, ref: ArchiveRef): Promise<string> {
  return fingerprint(await fs.readBytes(ref.root, ref.path));
}
