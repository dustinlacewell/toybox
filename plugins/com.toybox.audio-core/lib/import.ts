/**
 * The audio import, free of any user interface: copy the chosen sound files into
 * import staging and describe them as seed entries. The import panel and the
 * `tbx import audio` command both call it; the caller commits the entries.
 */

import { assertCategory, importNamespace, mergeImportOutcome, newImportOutcome, noteSkip, resolveSources, validSegment, type AudioSeedEntry, type FsApi, type ImportOutcome } from "@ldlework/toybox-sdk";

import { audioId, findAudio, supportedAudio } from "./files.js";

export type AudioImportOutcome = ImportOutcome<AudioSeedEntry>;

export interface AudioImportInput {
  /** The folder the files are read under. */
  source: string;
  /** Source-relative, forward-slashed paths of the files to import. */
  files: string[];
  /** Staging namespace, from `importNamespace`. */
  pack: string;
  category: string;
  /** Leave out a file that cannot be imported (and say why) instead of failing the whole import. */
  lenient?: boolean;
}

/** Copy `input.files` into `library/<pack>/<category>/` under `libraryRoot` and
 *  return the entries to commit. A file whose bytes are already there is not
 *  written again. */
export async function importAudioFiles(
  io: { fs: Pick<FsApi, "readBytes" | "writeBytes">; libraryRoot: string },
  input: AudioImportInput,
  progress?: (message: string) => void,
): Promise<AudioImportOutcome> {
  const { source, files, pack, category } = input;
  const outcome: AudioImportOutcome = newImportOutcome();
  let index = 0;
  for (const file of files) {
    progress?.(`Copying ${++index} of ${files.length}: ${file}`);
    try {
      if (file.split("/").some((segment) => !validSegment(segment))) throw new Error(`Cannot import this path: ${file}`);
      const bytes = await io.fs.readBytes(source, file);
      const target = `library/${pack}/${category}/${file}`;
      let existing: Uint8Array | undefined;
      try { existing = await io.fs.readBytes(io.libraryRoot, target); } catch { /* A new destination is expected. */ }
      if (!existing || !(existing.length === bytes.length && existing.every((value, at) => value === bytes[at]))) await io.fs.writeBytes(io.libraryRoot, target, bytes);
      outcome.entries.push({ kind: "audio", id: audioId(pack, category, file), pack, category, file, format: file.split(".").pop()?.toLowerCase() });
    } catch (failure) {
      if (!input.lenient) throw failure;
      const note = `skipped ${file}: ${failure instanceof Error ? failure.message : String(failure)}`;
      outcome.skipped += 1;
      outcome.skippedNotes.push(note);
      outcome.warnings.push(note);
    }
  }
  return outcome;
}

/** The whole import for paths given as files and folders: a folder is searched
 *  for WAV, MP3, Ogg and FLAC files the way the panel searches it; a named file
 *  that is not one of those is left out. */
export async function importAudioPaths(
  io: { fs: FsApi; libraryRoot: string },
  paths: string[],
  options: { category: string },
  progress?: (message: string) => void,
): Promise<AudioImportOutcome> {
  assertCategory(options.category);
  const total: AudioImportOutcome = newImportOutcome();
  for (const source of await resolveSources(io.fs, paths)) {
    const pack = importNamespace(source.root);
    let files = source.files ?? (await findAudio(io.fs, source.root));
    for (const file of files.filter((name) => !supportedAudio(name))) {
      noteSkip(total, `skipped ${file}: not a WAV, MP3, Ogg Vorbis or FLAC file`);
    }
    files = files.filter(supportedAudio);
    const outcome = await importAudioFiles(io, { source: source.root, files, pack, category: options.category, lenient: true }, progress);
    mergeImportOutcome(total, outcome);
  }
  return total;
}
