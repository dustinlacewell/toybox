/**
 * The Materials plugin on the command line: `tbx import material`. It shares
 * `lib/import.ts` with the import panel and has no user interface of its own.
 */

import { definePlugin, runImport } from "@ldlework/toybox-sdk";

import { importMaterialPaths } from "./lib/import.js";

export default definePlugin({
  cli: {
    importers: {
      material: (run) => runImport(
        run,
        (libraryRoot) => importMaterialPaths({ fs: run.fs, libraryRoot }, run.paths, { category: String(run.options.category ?? "uncategorized") }, run.log),
        (outcome) => `No materials found in ${run.paths.join(", ")}: a material needs at least an albedo (colour) image.${outcome.skippedNotes.length ? ` ${outcome.skippedNotes.join("; ")}` : ""}`,
      ),
    },
  },
});
