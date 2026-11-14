/**
 * Loads small preview images for the review screens. Each (path, size) is read
 * once and shared, at most a few reads run at a time, and the loader owns every
 * object URL it makes. No React.
 */

import type { FsApi } from "@ldlework/toybox-sdk";

const MAX_IN_FLIGHT = 4;

export interface ThumbLoader {
  /** An object URL for the image scaled to fit `size`; cached per (relPath,size). Rejects if the image cannot be decoded. */
  load(relPath: string, size: number): Promise<string>;
  /** Revoke every URL made. */
  dispose(): void;
}

export function createThumbLoader(
  fs: Pick<FsApi, "readImageThumb">,
  sourceRoot: string,
): ThumbLoader {
  const cache = new Map<string, Promise<string>>();
  const urls: string[] = [];
  const run = createLimiter(MAX_IN_FLIGHT);
  let disposed = false;

  const read = async (relPath: string, size: number): Promise<string> => {
    const bytes = await fs.readImageThumb(sourceRoot, relPath, size);
    const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: "image/png" }));
    if (disposed) {
      URL.revokeObjectURL(url);
      return url;
    }
    urls.push(url);
    return url;
  };

  return {
    load(relPath, size) {
      const key = `${size}:${relPath}`;
      let pending = cache.get(key);
      if (!pending) {
        pending = run(() => read(relPath, size));
        // A failed read is not cached, so a later call can try again.
        pending.catch(() => cache.delete(key));
        cache.set(key, pending);
      }
      return pending;
    },
    dispose() {
      disposed = true;
      for (const url of urls) URL.revokeObjectURL(url);
      urls.length = 0;
      cache.clear();
    },
  };
}

/** Runs tasks in the order given, at most `limit` at once. */
function createLimiter(limit: number): <T>(task: () => Promise<T>) => Promise<T> {
  let active = 0;
  const waiting: Array<() => void> = [];

  /** A finished task hands its slot straight to the next waiter, so a new caller
   *  cannot take it in between. */
  const release = () => {
    const waiter = waiting.shift();
    if (waiter) waiter();
    else active--;
  };

  return async <T>(task: () => Promise<T>): Promise<T> => {
    if (active >= limit) await new Promise<void>((resolve) => waiting.push(resolve));
    else active++;
    try {
      return await task();
    } finally {
      release();
    }
  };
}
