/**
 * Entry module of the glTF models plugin: contributes the glTF and GLB format to
 * the Models plugin's `model.formats` extension point.
 */

import { definePlugin, type PluginCtx } from "@ldlework/toybox-sdk";

import { createGltfFormat } from "./lib/gltf.js";

export default definePlugin((ctx: PluginCtx) => ({
  extensions: { "model.formats": [createGltfFormat(ctx.fs)] },
}));
