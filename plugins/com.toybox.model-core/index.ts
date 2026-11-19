/**
 * Entry module of the Models plugin, and also its command-line module (it has
 * no user interface). It contributes one value to its own `model.api`
 * extension point: the service other plugins (Animations) use to scan folders
 * for model files and convert them through the format plugins. It also
 * answers `tbx import model`. The import panel itself is `ui/Import.tsx`; both
 * call `lib/import.ts`.
 */

import { definePlugin, runImport, type PluginCtx } from "@ldlework/toybox-sdk";

import { MODEL_API, MODEL_FORMATS, type ModelFormat } from "./api.js";
import { collectFormats } from "./lib/formats.js";
import { importModelPaths } from "./lib/import.js";
import { createModelApi } from "./lib/service.js";

export default definePlugin((ctx: PluginCtx) => ({
  extensions: {
    [MODEL_API]: [createModelApi(ctx.fs, () => ctx.extensions<ModelFormat>(MODEL_FORMATS))],
  },
  cli: {
    importers: {
      model: (run) => runImport(
        run,
        (libraryRoot) => {
          const formats = collectFormats(run.extensions<ModelFormat>(MODEL_FORMATS));
          const levels = run.options.levels === "best" ? "best" : "all";
          return importModelPaths({ fs: run.fs, formats, libraryRoot }, run.paths, { category: String(run.options.category ?? "props"), levels }, run.log);
        },
        (outcome) => `No models found in ${run.paths.join(", ")}.${outcome.skippedNotes.length ? ` ${outcome.skippedNotes.join("; ")}` : ""}`,
      ),
    },
  },
}));
