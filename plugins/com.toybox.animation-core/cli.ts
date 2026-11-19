/**
 * The Animations plugin on the command line: `tbx import animation`. The
 * scanning and the file formats come from the Models plugin's `model.api`
 * service, whose own command-line module contributes it. No user interface;
 * the work is `lib/run.ts`, shared with the import panel.
 */

import { definePlugin, runImport } from "@ldlework/toybox-sdk";

import { findModelApi, MODEL_API } from "./lib/modelApi.js";
import { importAnimationPaths } from "./lib/paths.js";

export default definePlugin({
  cli: {
    importers: {
      animation: (run) => runImport(
        run,
        (libraryRoot) => {
          const api = findModelApi(run.extensions(MODEL_API));
          if (!api) throw new Error("The Models plugin is not available, so animations cannot be imported.");
          if (!api.formats().length) throw new Error(api.lookingFor());
          return importAnimationPaths(api, { fs: run.fs, libraryRoot }, run.paths, { category: String(run.options.category ?? "animations") }, run.log);
        },
        (outcome) => `No animation clips found in ${run.paths.join(", ")}.${outcome.skippedNotes.length ? ` ${outcome.skippedNotes.join("; ")}` : ""}`,
      ),
    },
  },
});
