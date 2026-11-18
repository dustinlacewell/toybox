/**
 * Library thumbnails: a still of the view from inside the sky. One renderer is
 * created on first use and shared, and captures run one at a time — browsers
 * allow only a handful of live WebGL contexts.
 */

import type { AssetFilesApi, PluginAssetView } from "@ldlework/toybox-sdk";

import { readMeta } from "./detect.js";
import { loadSky, type SkySource } from "./sky.js";
import { SkyStage } from "./stage.js";

/** Panoramas are shrunk to this width before upload: plenty for a small tile,
 *  and it keeps an 8K HDR from costing hundreds of megabytes per thumbnail. */
const THUMBNAIL_SOURCE_WIDTH = 2048;

let stage: SkyStage | undefined;
let queue: Promise<unknown> = Promise.resolve();

/** A sky's files as the loader wants them, read through the host. */
export function librarySource(asset: PluginAssetView, files: AssetFilesApi): SkySource {
  const names = asset.files.map((path) => path.slice(path.lastIndexOf("/") + 1));
  return {
    meta: readMeta(asset.meta, asset.files),
    file: names[0] ?? "",
    read: (file) => files.readFile(asset, file),
    url: (file) => files.fileUrl(asset, file),
  };
}

export function renderThumbnail(asset: PluginAssetView, files: AssetFilesApi, size: number): Promise<Uint8Array> {
  const run = queue.then(async () => {
    if (!stage) {
      const canvas = document.createElement("canvas");
      // Kept in the document, off screen: a detached canvas can lose its context.
      Object.assign(canvas.style, { position: "fixed", left: "-9999px", top: "0", pointerEvents: "none" });
      document.body.appendChild(canvas);
      stage = new SkyStage(canvas, { preserveDrawingBuffer: true });
    }
    const sky = await loadSky(librarySource(asset, files), { maxWidth: THUMBNAIL_SOURCE_WIDTH });
    try {
      stage.resize(size, size);
      stage.setSky(sky);
      stage.setExposure(0);
      stage.setSpheres(false);
      stage.setFov(85);
      stage.look(0, 0.12); // Slightly above the horizon: sky and ground both show.
      stage.render();
      const blob = await new Promise<Blob | null>((resolve) => stage!.renderer.domElement.toBlob(resolve, "image/png"));
      if (!blob) throw new Error("Could not capture the sky preview");
      return new Uint8Array(await blob.arrayBuffer());
    } finally {
      stage.setSky(undefined); // Free the texture; the renderer stays for the next one.
    }
  });
  queue = run.catch(() => undefined);
  return run;
}
