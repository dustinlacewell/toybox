/**
 * Library thumbnails of substances (window only; the command line never reaches
 * this module). A substance shows the material it makes by default (see
 * `defaultMaterial`), drawn by the host exactly as that material's own
 * thumbnail; without one, its `preview.png` colour on the same sphere; without
 * that, a neutral tile with the substance glyph.
 */

import type { AssetFilesApi, PluginAssetView, ThumbnailTools } from "@ldlework/toybox-sdk";

import { defaultMaterial } from "./graph.js";
import { hasPreview, PREVIEW_FILE } from "./substance.js";

/** The glyph of `icons/substance.svg`, on a 24-unit grid. */
const GLYPH = [
  "M6 7h8a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3v-8a3 3 0 0 1 3-3Z",
  "M3 15.5c2.5-2 5-2 7 0s4.5 2 7 0",
  "M7 7V5.5A2.5 2.5 0 0 1 9.5 3H13",
  "M19 2.5l.9 2.1 2.1.9-2.1.9-.9 2.1-.9-2.1-2.1-.9 2.1-.9Z",
];

function roundedRect(context: CanvasRenderingContext2D, x: number, y: number, width: number, height: number, radius: number): void {
  context.beginPath();
  context.moveTo(x + radius, y);
  context.arcTo(x + width, y, x + width, y + height, radius);
  context.arcTo(x + width, y + height, x, y + height, radius);
  context.arcTo(x, y + height, x, y, radius);
  context.arcTo(x, y, x + width, y, radius);
  context.closePath();
}

function glyph(context: CanvasRenderingContext2D, x: number, y: number, size: number, colour: string): void {
  context.save();
  context.translate(x, y);
  context.scale(size / 24, size / 24);
  context.strokeStyle = colour;
  context.lineWidth = 1.75;
  context.lineCap = "round";
  context.lineJoin = "round";
  for (const path of GLYPH) context.stroke(new Path2D(path));
  context.restore();
}

/** Draw the neutral tile of a substance that has nothing to show yet: `size` pixels, the glyph in the middle. */
export function drawPlaceholder(context: CanvasRenderingContext2D, size: number): void {
  const inset = Math.round(size * 0.08);
  const side = size - inset * 2;
  const radius = Math.round(size * 0.1);
  context.clearRect(0, 0, size, size);
  context.save();
  context.shadowColor = "rgba(0, 0, 0, 0.45)";
  context.shadowBlur = size * 0.05;
  context.shadowOffsetY = size * 0.02;
  roundedRect(context, inset, inset, side, side, radius);
  const ground = context.createLinearGradient(0, inset, 0, inset + side);
  ground.addColorStop(0, "#2d323c");
  ground.addColorStop(1, "#1b1e24");
  context.fillStyle = ground;
  context.fill();
  context.restore();
  const edge = Math.max(1, size / 128);
  roundedRect(context, inset + edge / 2, inset + edge / 2, side - edge, side - edge, radius - edge / 2);
  context.lineWidth = edge;
  context.strokeStyle = "rgba(255, 255, 255, 0.14)";
  context.stroke();
  glyph(context, size / 2 - side * 0.22, size / 2 - side * 0.22, side * 0.44, "#6b7484");
}

async function placeholder(size: number): Promise<Uint8Array> {
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("No 2D canvas to draw the thumbnail with");
  drawPlaceholder(context, size);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error("Could not capture the substance thumbnail");
  return new Uint8Array(await blob.arrayBuffer());
}

export async function renderThumbnail(asset: PluginAssetView, files: AssetFilesApi, size: number, tools?: ThumbnailTools): Promise<Uint8Array> {
  if (tools) {
    // The material it makes by default, drawn exactly as that material's own thumbnail.
    const material = defaultMaterial(await tools.derivedAssets(asset.id).catch(() => []));
    const shown = material && await tools.assetThumbnail(material.id, size).catch(() => undefined);
    if (shown) return shown;
    // None yet (or its maps cannot be had): its colour preview on the same sphere.
    if (hasPreview(asset)) {
      const sphere = await tools.materialSphere({ maps: [{ role: "albedo", file: PREVIEW_FILE }], resolveUrl: () => files.fileUrl(asset, PREVIEW_FILE) }, size).catch(() => undefined);
      if (sphere) return sphere;
    }
  }
  return placeholder(size);
}
