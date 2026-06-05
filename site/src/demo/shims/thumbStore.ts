/**
 * In-memory thumbnail store for the web demo. The real app persists rendered
 * PNGs to disk via Rust and reloads them through asset:// URLs; here the
 * thumbnail renderer (real app code) hands us PNG bytes and we keep them as
 * object URLs for the lifetime of the page. This is the only state the two
 * Tauri-seam shims share, factored out to avoid a circular import between them.
 */

const urlsById = new Map<string, string>();

/** Store rendered PNG bytes and return nothing — mirrors saveThumb's contract. */
export function putThumb(assetId: string, png: Uint8Array): void {
  const prev = urlsById.get(assetId);
  if (prev) URL.revokeObjectURL(prev);
  const blob = new Blob([png], { type: "image/png" });
  urlsById.set(assetId, URL.createObjectURL(blob));
}

/** The object URL for a rendered thumbnail, or null if it hasn't rendered yet. */
export function getThumbUrl(assetId: string): string | null {
  return urlsById.get(assetId) ?? null;
}
