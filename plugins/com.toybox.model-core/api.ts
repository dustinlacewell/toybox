/**
 * The contract of the `model.formats` extension point. Plugins that add a model
 * file format depend on `com.toybox.model-core`, import these types (type-only)
 * from its built declarations, and return their formats from their entry module:
 *
 *   extensions: { "model.formats": [myFormat] }
 *
 * The model importer reads them with `ctx.extensions<ModelFormat>(MODEL_FORMATS)`.
 * This plugin handles no file format itself: with none contributed it cannot
 * import anything.
 */

/** The extension point model format plugins contribute to. */
export const MODEL_FORMATS = "model.formats";

/** The extension point this plugin contributes to itself: one value, a
 *  `ModelApi`, for the plugins that need to read model files (animations).
 *  Plugins cannot import each other at run time, so the host carries it. */
export const MODEL_API = "model.api";

/** One file the user's chosen folder holds, in a format the plugin handles. */
export interface ModelSource {
  /** The folder the user picked. */
  root: string;
  /** File path relative to `root`, forward slashes. */
  path: string;
  /** File name without extension; the model's name. */
  stem: string;
}

/** Where the converted model goes. */
export interface ModelTarget {
  /** The library folder that holds `library/<pack>/<category>/`. */
  libraryRoot: string;
  pack: string;
  category: string;
}

/** A converted file and any notes worth showing the user, or the reason the
 *  file cannot be imported.
 *
 *  `originals` names files that belong to the source file and are needed to
 *  read it again, such as the material files an OBJ refers to: paths relative to
 *  `ModelSource.root`, forward slashes, only files the import actually read.
 *  Do not list the source file itself, nor images the converter already copied
 *  beside the glTF. The importer keeps the source file and these beside the
 *  converted model (see "Keeping the original" in docs/models.md). */
export type ModelImportResult = { file: string; warnings: string[]; originals?: string[] } | { rejected: string };

/** A model file format a plugin can import. */
export interface ModelFormat {
  /** Short stable id, e.g. "fbx". */
  id: string;
  /** Name shown to the user, e.g. "FBX". */
  label: string;
  /** File extensions handled, lower case, without the dot. */
  extensions: string[];
  /** Whether the importer keeps the source file beside the converted glTF as
   *  the model's original. Default true. A format whose output already is the
   *  stored file (glTF) sets this to false; the entry then records only which
   *  plugin imported it. */
  keepsOriginal?: boolean;
  /** Convert one source file into the library's model form: a loose glTF at
   *  `library/<pack>/<category>/<stem>.gltf` under `libraryRoot`, whose single
   *  buffer is `<stem>.bin` beside it, with its texture images beside it too.
   *  Returns the `.gltf` file name. Return `{ rejected }` with a plain reason
   *  when the file cannot be imported; a thrown error is treated the same way. */
  import(source: ModelSource, target: ModelTarget): Promise<ModelImportResult>;
}

/** A model file found under a folder, in a format that is turned on. */
export interface ModelFile {
  /** Path relative to the folder that was scanned, forward slashes. */
  relPath: string;
  /** File name without extension. */
  stem: string;
  /** Lower-case extension without the dot. */
  ext: string;
}

/** The service published under `model.api`. It is built inside this plugin, so
 *  it runs with this plugin's file permissions whoever calls it, and it reads the
 *  formats that are turned on at the moment of each call. */
export interface ModelApi {
  /** The formats contributed by the plugins that are on, in the order claimed. */
  formats(): { id: string; label: string; extensions: string[] }[];
  /** "Looks for .gltf and .glb files.", or, when `formats()` is empty, the
   *  sentence that tells the user to turn on a format plugin. */
  lookingFor(): string;
  /** Every file under `sourceRoot` (a folder the user picked) that a turned-on
   *  format handles. Empty when no format is on. */
  scan(sourceRoot: string): Promise<ModelFile[]>;
  /** Convert one scanned file with its format into the loose glTF
   *  `library/<pack>/<category>/<stem>.gltf` under `libraryRoot`. Keeps no
   *  original and returns the `.gltf` file name, or why the file was skipped. */
  convert(sourceRoot: string, libraryRoot: string, file: ModelFile, pack: string, category: string): Promise<{ file: string; warnings: string[] } | { rejected: string }>;
  /** Whether the converted glTF `file` holds at least one animation clip with channels. */
  hasAnimationClips(libraryRoot: string, pack: string, category: string, file: string): Promise<boolean>;
}
