/**
 * Source-tree discovery and per-asset materialization. Pure orchestration over
 * the host's jailed fs: walk a picked source root, find every `.gltf`/`.glb`,
 * and copy/unpack each into the library layout. Kept free of React so the import
 * logic is testable and reads top-down.
 */

import type { FsApi } from "@ldlework/toybox-sdk";
import { unpackGlb, type GlbReject } from "./glb.js";

/** A model file discovered under the source root. */
export interface Found {
  /** Source-root-relative path (forward-slashed), as returned by readDir. */
  relPath: string;
  /** Basename without extension — the asset name and target stem. */
  stem: string;
  kind: "gltf" | "glb";
}

/** Recursively list every `.gltf`/`.glb` under `sourceRoot`. */
export async function findModels(fs: FsApi, sourceRoot: string): Promise<Found[]> {
  const out: Found[] = [];
  const queue: string[] = ["."];
  while (queue.length) {
    const dir = queue.shift()!;
    for (const e of await fs.readDir(sourceRoot, dir)) {
      if (e.isDir) {
        queue.push(e.relPath);
        continue;
      }
      const kind = modelKind(e.name);
      if (kind) out.push({ relPath: e.relPath, stem: stemOf(e.name), kind });
    }
  }
  return out;
}

/** The outcome of materializing one found model into the library. */
export interface Materialized {
  /** Library-relative `.gltf` filename the seed entry references. */
  file: string;
  /** Human-facing note (texture skips, embedded-image rejects) for the report. */
  warnings: string[];
}

/** A rejection reason that also covers loose-glTF shapes the library can't hold. */
export type Reject = GlbReject | "multi-buffer" | "bad-gltf";

/**
 * Copy/unpack one found model into `library/<pack>/<category>/` under the library
 * root, normalizing it to the library invariant the index assumes: a loose glTF
 * whose single buffer is `<stem>.bin`, sitting beside its `.bin` and textures.
 *
 * The native index (`entry_from_parts`) hardcodes the catalog's bin to
 * `<stem>.bin` regardless of what the source named it, so we must rename the
 * buffer to match — otherwise the viewer and exporters resolve a bin the catalog
 * doesn't know. Both source kinds funnel through the same rename, so glb and
 * loose produce byte-identical library shapes.
 */
export async function materialize(
  fs: FsApi,
  sourceRoot: string,
  libraryRoot: string,
  found: Found,
  pack: string,
  category: string,
): Promise<Materialized | { rejected: Reject }> {
  const destDir = `library/${pack}/${category}`;
  const gltfFile = `${found.stem}.gltf`;
  const warnings: string[] = [];

  // Normalize source → { gltf doc with buffer uri = <stem>.bin, bin bytes }.
  const normalized = await normalize(fs, sourceRoot, found, warnings);
  if ("rejected" in normalized) return normalized;
  const { doc, bin } = normalized;

  await fs.writeText(libraryRoot, `${destDir}/${gltfFile}`, JSON.stringify(doc, null, 2));
  await fs.writeBytes(libraryRoot, `${destDir}/${found.stem}.bin`, bin);
  // Textures keep their relative shape so the gltf's own uris still resolve.
  const srcDir = dirOf(found.relPath);
  await copyReferenced(fs, sourceRoot, libraryRoot, srcDir, destDir, imageUris(doc), warnings);

  return { file: gltfFile, warnings };
}

/** Bring either source kind to the canonical shape: a parsed glTF doc whose
 *  single buffer uri is `<stem>.bin`, plus that buffer's bytes. */
async function normalize(
  fs: FsApi,
  sourceRoot: string,
  found: Found,
  warnings: string[],
): Promise<{ doc: GltfDoc; bin: Uint8Array } | { rejected: Reject }> {
  if (found.kind === "glb") {
    const res = unpackGlb(await fs.readBytes(sourceRoot, found.relPath), found.stem);
    if (!res.ok) return { rejected: res.reason };
    return { doc: JSON.parse(res.value.gltfText) as GltfDoc, bin: res.value.bin };
  }

  let doc: GltfDoc;
  try {
    doc = JSON.parse(new TextDecoder().decode(await fs.readBytes(sourceRoot, found.relPath)));
  } catch {
    return { rejected: "bad-gltf" };
  }
  if ((doc.buffers?.length ?? 0) !== 1) return { rejected: "multi-buffer" };

  const srcDir = dirOf(found.relPath);
  const srcUri = doc.buffers![0].uri;
  // A data-uri buffer means the bin is inline; out of scope for v1.
  if (!srcUri || srcUri.startsWith("data:")) return { rejected: "bad-gltf" };

  let bin: Uint8Array;
  try {
    bin = await fs.readBytes(sourceRoot, joinRel(srcDir, srcUri));
  } catch {
    warnings.push(`missing buffer: ${srcUri}`);
    return { rejected: "bad-gltf" };
  }
  // Rename the buffer to the <stem>.bin the catalog will record.
  doc.buffers![0] = { ...doc.buffers![0], uri: `${found.stem}.bin` };
  return { doc, bin };
}

/** Copy each relative uri (resolved against the gltf's source dir) into destDir,
 *  preserving the uri's own relative shape so the glTF's paths still resolve. */
async function copyReferenced(
  fs: FsApi,
  sourceRoot: string,
  libraryRoot: string,
  srcDir: string,
  destDir: string,
  uris: string[],
  warnings: string[],
): Promise<void> {
  for (const uri of uris) {
    if (uri.startsWith("data:") || /^https?:\/\//.test(uri)) continue;
    const srcRel = joinRel(srcDir, uri);
    const dstRel = joinRel(destDir, uri);
    try {
      const bytes = await fs.readBytes(sourceRoot, srcRel);
      await fs.writeBytes(libraryRoot, dstRel, bytes);
    } catch {
      warnings.push(`missing referenced file: ${uri}`);
    }
  }
}

// --- pure helpers ------------------------------------------------------------

function modelKind(name: string): "gltf" | "glb" | null {
  const lower = name.toLowerCase();
  if (lower.endsWith(".gltf")) return "gltf";
  if (lower.endsWith(".glb")) return "glb";
  return null;
}

function stemOf(name: string): string {
  return name.replace(/\.[^.]+$/, "");
}

function dirOf(relPath: string): string {
  const i = relPath.lastIndexOf("/");
  return i < 0 ? "" : relPath.slice(0, i);
}

/** Resolve a relative uri against a dir, collapsing `.`/`..` — mirrors the
 *  host's `resolve_uri_rel`. Both inputs are forward-slashed. */
function joinRel(dir: string, uri: string): string {
  const stack = dir ? dir.split("/").filter(Boolean) : [];
  for (const seg of uri.split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") stack.pop();
    else stack.push(seg);
  }
  return stack.join("/");
}

function imageUris(doc: GltfDoc): string[] {
  return (doc.images ?? []).map((i) => i.uri).filter((u): u is string => !!u);
}

interface GltfDoc {
  buffers?: { uri?: string; byteLength?: number }[];
  images?: { uri?: string }[];
}
