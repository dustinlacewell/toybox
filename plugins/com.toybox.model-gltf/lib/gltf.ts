/**
 * The glTF and GLB format of the model importer. A model enters the library as
 * a loose glTF whose single buffer is `<stem>.bin`, beside its `.bin` and
 * textures. A `.gltf` is copied (its buffer renamed to match), a `.glb` is
 * split into the same shape. Nothing is converted, so no original is kept: the
 * stored file is the original.
 *
 * The native index (`entry_from_parts`) hardcodes the catalog's bin to
 * `<stem>.bin` regardless of what the source named it, hence the rename.
 * Pure orchestration over the jailed fs it is given, so it is testable.
 */

import type { FsApi } from "@ldlework/toybox-sdk";
import type { ModelFormat, ModelSource, ModelTarget } from "com.toybox.model-core";
import { unpackGlb } from "./glb.js";

export function createGltfFormat(fs: FsApi): ModelFormat {
  return {
    id: "gltf",
    label: "glTF",
    extensions: ["gltf", "glb"],
    keepsOriginal: false,
    async import(source, target) {
      const warnings: string[] = [];
      const normalized = await normalize(fs, source, warnings);
      if ("rejected" in normalized) return normalized;
      const { doc, bin } = normalized;

      const destDir = `library/${target.pack}/${target.category}`;
      await fs.writeText(target.libraryRoot, `${destDir}/${source.stem}.gltf`, JSON.stringify(doc, null, 2));
      await fs.writeBytes(target.libraryRoot, `${destDir}/${source.stem}.bin`, bin);
      // Textures keep their relative shape so the gltf's own uris still resolve.
      await copyReferenced(fs, source, target, dirOf(source.path), destDir, imageUris(doc), warnings);
      return { file: `${source.stem}.gltf`, warnings };
    },
  };
}

/** Bring either source kind to the canonical shape: a parsed glTF doc whose
 *  single buffer uri is `<stem>.bin`, plus that buffer's bytes. A file that cannot be
 *  used is refused with a short reason for the import summary. */
async function normalize(
  fs: FsApi,
  source: ModelSource,
  warnings: string[],
): Promise<{ doc: GltfDoc; bin: Uint8Array } | { rejected: string }> {
  if (extensionOf(source.path) === "glb") {
    const res = unpackGlb(await fs.readBytes(source.root, source.path), source.stem);
    if (!res.ok) return { rejected: res.reason };
    return { doc: JSON.parse(res.value.gltfText) as GltfDoc, bin: res.value.bin };
  }

  let doc: GltfDoc;
  try {
    doc = JSON.parse(new TextDecoder().decode(await fs.readBytes(source.root, source.path)));
  } catch {
    return { rejected: "bad-gltf" };
  }
  if ((doc.buffers?.length ?? 0) !== 1) return { rejected: "multi-buffer" };

  const srcDir = dirOf(source.path);
  const srcUri = doc.buffers![0].uri;
  // A data-uri buffer means the bin is inline; out of scope.
  if (!srcUri || srcUri.startsWith("data:")) return { rejected: "bad-gltf" };

  let bin: Uint8Array;
  try {
    bin = await fs.readBytes(source.root, joinRel(srcDir, srcUri));
  } catch {
    warnings.push(`missing buffer: ${srcUri}`);
    return { rejected: "bad-gltf" };
  }
  // Rename the buffer to the <stem>.bin the catalog will record.
  doc.buffers![0] = { ...doc.buffers![0], uri: `${source.stem}.bin` };
  return { doc, bin };
}

/** Copy each relative uri (resolved against the gltf's source dir) into destDir,
 *  preserving the uri's own relative shape so the glTF's paths still resolve. */
async function copyReferenced(
  fs: FsApi,
  source: ModelSource,
  target: ModelTarget,
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
      const bytes = await fs.readBytes(source.root, srcRel);
      await fs.writeBytes(target.libraryRoot, dstRel, bytes);
    } catch {
      warnings.push(`missing referenced file: ${uri}`);
    }
  }
}

// --- pure helpers ------------------------------------------------------------

function extensionOf(path: string): string {
  const i = path.lastIndexOf(".");
  return i <= 0 ? "" : path.slice(i + 1).toLowerCase();
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
