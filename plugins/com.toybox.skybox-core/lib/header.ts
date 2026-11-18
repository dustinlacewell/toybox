/**
 * Image dimensions read straight from a file's header bytes, for every format
 * a sky can be stored in. Pure — no decoding, no DOM — so an importer can size
 * a 200 MB panorama without decompressing it.
 */

export interface ImageSize { width: number; height: number; }

const ascii = (bytes: Uint8Array, start: number, length: number) => String.fromCharCode(...bytes.subarray(start, start + length));

/** The pixel size of an HDR, EXR, PNG, JPEG or WebP image, or undefined when
 *  the bytes are not one of those or the header is damaged. */
export function imageSize(bytes: Uint8Array): ImageSize | undefined {
  try {
    const size = radiance(bytes) ?? openExr(bytes) ?? png(bytes) ?? jpeg(bytes) ?? webp(bytes);
    return size && size.width > 0 && size.height > 0 && Number.isFinite(size.width) && Number.isFinite(size.height) ? size : undefined;
  } catch {
    return undefined; // A truncated header reads past the end; that is "unknown".
  }
}

/** Radiance `.hdr`: text header lines, a blank line, then `-Y <h> +X <w>`. */
function radiance(bytes: Uint8Array): ImageSize | undefined {
  if (ascii(bytes, 0, 2) !== "#?") return undefined;
  const head = ascii(bytes, 0, Math.min(bytes.length, 4096));
  const match = /\n\s*([-+][XY])\s+(\d+)\s+([-+][XY])\s+(\d+)\s*\n/.exec(head);
  if (!match) return undefined;
  const first = Number(match[2]), second = Number(match[4]);
  // The first axis listed is the scanline direction; Y-first is the usual order.
  return match[1].endsWith("Y") ? { width: second, height: first } : { width: first, height: second };
}

/** OpenEXR: magic, version, then named attributes; `dataWindow` is a box2i. */
function openExr(bytes: Uint8Array): ImageSize | undefined {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (bytes.length < 8 || view.getUint32(0, true) !== 20000630) return undefined;
  const text = (start: number) => { let end = start; while (bytes[end] !== 0) { if (++end >= bytes.length) throw new Error("truncated"); } return { value: ascii(bytes, start, end - start), next: end + 1 }; };
  let offset = 8;
  // Attributes end at an empty name. Only the first part's header is needed.
  for (let guard = 0; guard < 256; guard++) {
    const name = text(offset);
    if (!name.value) return undefined;
    const type = text(name.next);
    const size = view.getInt32(type.next, true);
    const data = type.next + 4;
    if (name.value === "dataWindow" && type.value === "box2i" && size === 16) {
      const xMin = view.getInt32(data, true), yMin = view.getInt32(data + 4, true), xMax = view.getInt32(data + 8, true), yMax = view.getInt32(data + 12, true);
      return { width: xMax - xMin + 1, height: yMax - yMin + 1 };
    }
    if (size < 0) return undefined;
    offset = data + size;
  }
  return undefined;
}

function png(bytes: Uint8Array): ImageSize | undefined {
  if (bytes.length < 24 || bytes[0] !== 0x89 || ascii(bytes, 1, 3) !== "PNG" || ascii(bytes, 12, 4) !== "IHDR") return undefined;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return { width: view.getUint32(16), height: view.getUint32(20) };
}

/** JPEG: walk the segments to the first start-of-frame marker. */
function jpeg(bytes: Uint8Array): ImageSize | undefined {
  if (bytes[0] !== 0xff || bytes[1] !== 0xd8) return undefined;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let offset = 2;
  while (offset + 9 < bytes.length) {
    if (bytes[offset] !== 0xff) return undefined;
    const marker = bytes[offset + 1];
    if (marker === 0xff) { offset++; continue; } // Fill byte.
    // SOF0–SOF15, except DHT (C4), JPG (C8) and DAC (CC), hold the frame size.
    if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
      return { width: view.getUint16(offset + 7), height: view.getUint16(offset + 5) };
    }
    offset += 2 + view.getUint16(offset + 2);
  }
  return undefined;
}

/** WebP: a RIFF container whose first chunk is VP8 (lossy), VP8L (lossless) or
 *  VP8X (extended). */
function webp(bytes: Uint8Array): ImageSize | undefined {
  if (bytes.length < 30 || ascii(bytes, 0, 4) !== "RIFF" || ascii(bytes, 8, 4) !== "WEBP") return undefined;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const chunk = ascii(bytes, 12, 4);
  if (chunk === "VP8X") return { width: 1 + (bytes[24] | (bytes[25] << 8) | (bytes[26] << 16)), height: 1 + (bytes[27] | (bytes[28] << 8) | (bytes[29] << 16)) };
  if (chunk === "VP8L") { const bits = view.getUint32(21, true); return { width: (bits & 0x3fff) + 1, height: ((bits >> 14) & 0x3fff) + 1 }; }
  if (chunk === "VP8 ") return { width: view.getUint16(26, true) & 0x3fff, height: view.getUint16(28, true) & 0x3fff };
  return undefined;
}
