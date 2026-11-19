/**
 * The Skyboxes plugin on the command line: `tbx import skybox`. The entry
 * module imports React and three.js for the viewer, so the command line has this
 * module of its own; the work is `lib/import.ts`, shared with the import panel.
 */

import { definePlugin, runImport } from "@ldlework/toybox-sdk";

import { IMAGE_LAYOUTS, importSkyPaths, type LayoutChoice } from "./lib/import.js";

export default definePlugin({
  cli: {
    importers: {
      skybox: (run) => runImport(
        run,
        (libraryRoot) => {
          const layout = String(run.options.layout ?? "auto");
          if (layout !== "auto" && !IMAGE_LAYOUTS.includes(layout as never)) throw new Error(`Unknown layout ${layout}.`);
          return importSkyPaths(
            { fs: run.fs, libraryRoot },
            run.paths,
            { category: String(run.options.category ?? "skies"), layout: layout as LayoutChoice, all: run.options.all === true },
            run.log,
          );
        },
        (outcome) => `No skies found in ${run.paths.join(", ")}: looked for HDR, EXR, PNG, JPG and WebP images.${outcome.skippedNotes.length ? ` ${outcome.skippedNotes.join("; ")}` : ""}`,
      ),
    },
  },
});
