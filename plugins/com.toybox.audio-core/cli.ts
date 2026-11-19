/**
 * The Audio plugin on the command line: `tbx import audio`. It shares
 * `lib/import.ts` with the import panel and has no user interface of its own.
 */

import { definePlugin, runImport } from "@ldlework/toybox-sdk";

import { importAudioPaths } from "./lib/import.js";

export default definePlugin({
  cli: {
    importers: {
      audio: (run) => runImport(
        run,
        (libraryRoot) => importAudioPaths({ fs: run.fs, libraryRoot }, run.paths, { category: String(run.options.category ?? "sounds") }, run.log),
        (outcome) => `No WAV, MP3, Ogg Vorbis or FLAC files found in ${run.paths.join(", ")}${outcome.skippedNotes.length ? ` (${outcome.skippedNotes.join("; ")})` : ""}.`,
      ),
    },
  },
});
