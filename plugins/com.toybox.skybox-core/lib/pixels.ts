/**
 * Raw pixel-grid operations for high-dynamic-range images: flipping, shrinking
 * and cutting an unfolded cube into faces. Pure typed-array work — no three or
 * DOM — so it is unit-tested directly. Rows are always top first here.
 */

import type { FaceCell } from "./detect.js";

export type Samples = Float32Array | Uint16Array;
export interface PixelGrid<T extends Samples = Samples> {
  data: T;
  width: number;
  height: number;
  /** Samples per pixel. */
  channels: number;
}

const like = <T extends Samples>(source: T, length: number): T => new (source.constructor as { new(length: number): T })(length);

/** Reverse the row order (bottom-first ⇄ top-first). */
export function flipRows<T extends Samples>(grid: PixelGrid<T>): PixelGrid<T> {
  const { data, width, height, channels } = grid;
  const row = width * channels;
  const out = like(data, data.length);
  for (let y = 0; y < height; y++) out.set(data.subarray(y * row, (y + 1) * row), (height - 1 - y) * row);
  return { ...grid, data: out };
}

/** Shrink by a whole-number step so the width is at most `maxWidth`. Sampling
 *  is nearest-pixel: previews want speed and bounded memory, not filtering. */
export function shrink<T extends Samples>(grid: PixelGrid<T>, maxWidth: number): PixelGrid<T> {
  const step = Math.ceil(grid.width / Math.max(1, maxWidth));
  if (step <= 1) return grid;
  const width = Math.max(1, Math.floor(grid.width / step)), height = Math.max(1, Math.floor(grid.height / step));
  const { data, channels } = grid;
  const out = like(data, width * height * channels);
  for (let y = 0; y < height; y++) {
    const from = y * step * grid.width * channels, to = y * width * channels;
    for (let x = 0; x < width; x++) {
      const source = from + x * step * channels, target = to + x * channels;
      for (let c = 0; c < channels; c++) out[target + c] = data[source + c];
    }
  }
  return { data: out, width, height, channels };
}

/** Cut one cell out of an unfolded cube image. `turn` rotates it by 180°. */
export function cutCell<T extends Samples>(grid: PixelGrid<T>, columns: number, rows: number, cell: FaceCell): PixelGrid<T> {
  const width = Math.floor(grid.width / columns), height = Math.floor(grid.height / rows);
  const { data, channels } = grid;
  const out = like(data, width * height * channels);
  const left = cell.column * width, top = cell.row * height;
  for (let y = 0; y < height; y++) {
    const from = ((top + y) * grid.width + left) * channels;
    if (!cell.turn) { out.set(data.subarray(from, from + width * channels), y * width * channels); continue; }
    const to = (height - 1 - y) * width * channels;
    for (let x = 0; x < width; x++) {
      const source = from + x * channels, target = to + (width - 1 - x) * channels;
      for (let c = 0; c < channels; c++) out[target + c] = data[source + c];
    }
  }
  return { data: out, width, height, channels };
}
