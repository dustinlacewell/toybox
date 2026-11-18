/**
 * The Skyboxes asset type. This plugin is everything Toybox knows about skies:
 * the manifest declares the type, `ui/Import` brings skies into the library,
 * and this module supplies their viewer, thumbnails and properties. The host
 * stores, lists, filters and exports them like any other asset.
 */

import { defineAssetTypes } from "@ldlework/toybox-sdk";

import { LAYOUT_LABELS, describeSize, isHdr, readMeta } from "./lib/detect.js";
import { renderThumbnail } from "./lib/thumbnail.js";
import { SkyViewer } from "./ui/Viewer.js";

const SHORT_LAYOUT = { equirect: "Panorama", cubemap: "Cube map", "cross-horizontal": "Cube cross", "cross-vertical": "Cube cross", "strip-horizontal": "Cube strip", "strip-vertical": "Cube strip" } as const;

export default defineAssetTypes({
  types: {
    skybox: {
      Viewer: SkyViewer,
      renderThumbnail,
      subtitle: (asset) => {
        const meta = readMeta(asset.meta, asset.files);
        return `${SHORT_LAYOUT[meta.layout]} · ${meta.format}`;
      },
      properties: (asset) => {
        const meta = readMeta(asset.meta, asset.files);
        return [
          { label: "Layout", value: LAYOUT_LABELS[meta.layout] },
          { label: "Format", value: meta.format },
          { label: "Size", value: describeSize(meta) },
          { label: "Light range", value: isHdr(meta.encoding) ? "High dynamic range" : "Standard" },
        ];
      },
    },
  },
});
