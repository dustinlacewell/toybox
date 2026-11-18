/**
 * Entry module of the Models plugin. It contributes one value to its own
 * `model.api` extension point: the service other plugins (Animations) use to
 * scan folders for model files and convert them through the format plugins.
 * The import panel itself is `ui/Import.tsx`.
 */

import { definePlugin, type PluginCtx } from "@ldlework/toybox-sdk";

import { MODEL_API, MODEL_FORMATS, type ModelFormat } from "./api.js";
import { createModelApi } from "./lib/service.js";

export default definePlugin((ctx: PluginCtx) => ({
  extensions: {
    [MODEL_API]: [createModelApi(ctx.fs, () => ctx.extensions<ModelFormat>(MODEL_FORMATS))],
  },
}));
