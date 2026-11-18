/**
 * GLB → loose-glTF unpacking, in plain JS (no three.js). A `.glb` is a binary
 * container; the library indexes loose separate-format glTF + sidecar `.bin`, so
 * a `.glb` source must be split back out before it can enter.
 *
 * GLB layout (little-endian) — the exact inverse of the host's assembler:
 *   header: magic 0x46546C67 ("glTF"), version 2, total length (u32)
 *   chunk 0: JSON — length, type 0x4E4F534A, data padded with 0x20
 *   chunk 1: BIN  — length, type 0x004E4942, data padded with 0x00
 *
 * v1 scope: assets whose images are external (`images[].uri`) or absent unpack
 * cleanly — we rewrite the single buffer's uri to `<name>.bin` and emit both
 * files. Assets with bufferView-backed (embedded) images are NOT handled here;
 * the caller is told so and skips them rather than emitting a broken asset.
 */

const MAGIC = 0x46546c67;
const CHUNK_JSON = 0x4e4f534a;
const CHUNK_BIN = 0x004e4942;

export interface UnpackedGlb {
  /** The loose glTF document text, buffer uri rewritten to `<stem>.bin`. */
  gltfText: string;
  /** The single buffer's bytes, to write as `<stem>.bin`. */
  bin: Uint8Array;
}

/** Why a `.glb` couldn't be unpacked to loose glTF for import. */
export type GlbReject =
  | "not-a-glb"
  | "unsupported-version"
  | "no-bin-chunk"
  | "multi-buffer"
  | "embedded-images";

export type UnpackResult =
  | { ok: true; value: UnpackedGlb }
  | { ok: false; reason: GlbReject };

/** Split a `.glb` into loose glTF text + bin bytes, or reject with a reason. */
export function unpackGlb(bytes: Uint8Array, stem: string): UnpackResult {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (bytes.byteLength < 12 || view.getUint32(0, true) !== MAGIC) {
    return { ok: false, reason: "not-a-glb" };
  }
  if (view.getUint32(4, true) !== 2) {
    return { ok: false, reason: "unsupported-version" };
  }

  const chunks = readChunks(bytes, view);
  const jsonChunk = chunks.find((c) => c.type === CHUNK_JSON);
  const binChunk = chunks.find((c) => c.type === CHUNK_BIN);
  if (!jsonChunk) return { ok: false, reason: "not-a-glb" };
  if (!binChunk) return { ok: false, reason: "no-bin-chunk" };

  const doc = JSON.parse(new TextDecoder().decode(jsonChunk.data)) as GltfDoc;

  // The library's model is single-buffer separate-format; a multi-buffer glb is
  // outside that shape, and an embedded image would be lost by a bare bin split.
  if ((doc.buffers?.length ?? 0) !== 1) return { ok: false, reason: "multi-buffer" };
  if (hasEmbeddedImages(doc)) return { ok: false, reason: "embedded-images" };

  // The buffer's true (unpadded) length is the authoritative one in the source
  // JSON; the BIN chunk is zero-padded to a 4-byte boundary, so trim to it. Fall
  // back to the chunk length only if the source omitted byteLength.
  const byteLength = doc.buffers![0].byteLength ?? binChunk.data.byteLength;
  const bin = binChunk.data.subarray(0, byteLength);

  // Point the buffer at the sidecar we're about to write; drop any embed marker.
  doc.buffers![0] = { uri: `${stem}.bin`, byteLength };

  return { ok: true, value: { gltfText: JSON.stringify(doc, null, 2), bin } };
}

interface Chunk {
  type: number;
  data: Uint8Array;
}

function readChunks(bytes: Uint8Array, view: DataView): Chunk[] {
  const chunks: Chunk[] = [];
  let off = 12; // past the 12-byte header
  while (off + 8 <= bytes.byteLength) {
    const len = view.getUint32(off, true);
    const type = view.getUint32(off + 4, true);
    const start = off + 8;
    chunks.push({ type, data: bytes.subarray(start, start + len) });
    off = start + len;
  }
  return chunks;
}

/** True if any image is backed by a bufferView (embedded) rather than a uri. */
function hasEmbeddedImages(doc: GltfDoc): boolean {
  return (doc.images ?? []).some(
    (img) => img.bufferView !== undefined || (img.uri?.startsWith("data:") ?? false),
  );
}

interface GltfDoc {
  buffers?: { uri?: string; byteLength?: number }[];
  images?: { uri?: string; bufferView?: number }[];
}
