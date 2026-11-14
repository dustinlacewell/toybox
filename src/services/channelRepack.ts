/**
 * Copy one channel of an image into all of r, g and b of a new texture, so
 * whichever channel three samples for a slot reads the wanted value. Used for
 * scalar maps stored in another channel or inverted.
 */

import { CanvasTexture, LinearFilter, LinearMipmapLinearFilter } from "three";

import type { MapChannel } from "../domain/material";

type Source = CanvasImageSource & { width: number; height: number };

const CHANNEL_OFFSET: Record<MapChannel, number> = { r: 0, g: 1, b: 2, a: 3 };

/**
 * Build a grayscale texture from one channel of `image`. `from` undefined reads
 * red. The long side is downscaled to at most `maxSize`. Colour space stays
 * linear (it is data, not colour).
 */
export function repackChannel(
  image: Source,
  from: MapChannel | undefined,
  invert: boolean,
  maxSize: number,
): CanvasTexture {
  const { width, height } = fitSize(image.width, image.height, maxSize);
  const ctx = createContext(width, height);
  ctx.drawImage(image, 0, 0, width, height);

  const pixels = ctx.getImageData(0, 0, width, height);
  writeGray(pixels.data, CHANNEL_OFFSET[from ?? "r"], invert);
  ctx.putImageData(pixels, 0, 0);

  const texture = new CanvasTexture(ctx.canvas);
  texture.magFilter = LinearFilter;
  texture.minFilter = LinearMipmapLinearFilter;
  return texture;
}

function fitSize(w: number, h: number, maxSize: number): { width: number; height: number } {
  const scale = Math.min(1, maxSize / Math.max(w, h));
  return {
    width: Math.max(1, Math.round(w * scale)),
    height: Math.max(1, Math.round(h * scale)),
  };
}

function createContext(width: number, height: number): CanvasRenderingContext2D {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) throw new Error("channelRepack: no 2D canvas context");
  return ctx;
}

/** Rewrite RGBA pixels in place: r = g = b = the source channel (maybe inverted), a = 255. */
function writeGray(data: Uint8ClampedArray, offset: number, invert: boolean): void {
  for (let i = 0; i < data.length; i += 4) {
    const v = data[i + offset];
    const out = invert ? 255 - v : v;
    data[i] = out;
    data[i + 1] = out;
    data[i + 2] = out;
    data[i + 3] = 255;
  }
}
