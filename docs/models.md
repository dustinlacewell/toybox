# Models and animations

Models and animations come from three bundled plugins, each with one
responsibility. The library only ever holds one model form, loose glTF (see
[library-format-spec.md](library-format-spec.md)); a format plugin turns a file
format into it.

| Plugin | Id | Does |
| --- | --- | --- |
| **Models** | `com.toybox.model-core` | Declares the Models library type and owns its import panel: pick a folder, scan, choose, hand each file to a format, keep originals, commit. Publishes the `model.formats` extension point formats plug into and the `model.api` service. It reads **no file format itself**. |
| **glTF models** | `com.toybox.model-gltf` | Contributes `.gltf` and `.glb` to `model.formats`. |
| **Animations** | `com.toybox.animation-core` | Declares the Animations library type and owns its import panel. Clips are taken from model files, so it scans and converts them through the `model.api` service of **Models**. |

**glTF models** and **Animations** list `com.toybox.model-core` under
`dependencies` (`^3.0.0`), so turning one on turns **Models** on, and **Models**
cannot be turned off while either is on.

With **Models** on and no format plugin on, the import panel offers no scan. It
says "No model formats are turned on. Turn on a format plugin such as glTF models
in Settings → Plugins." The Animations panel says the same. Turn on **glTF
models** and the hint under **Choose source folder** reads "Looks for .gltf and
.glb files."; every further format adds its extension to it.

The rigs that animations are retargeted between are separate plugins that depend
on **Models** (see [Rig adapters](rig-adapters.md)), and the whole bundled set is
listed in [Plugins](plugins.md#the-bundled-plugins).

FBX and OBJ import and export are not bundled. The host still carries what an
FBX format plugin needs: the `convertToGltf` host call (permission
`rustConvert`) and the FBX2glTF path preference.

## How the pieces fit

The import panel belongs to **Models**. When you choose a folder it walks it,
and for every file it hands the work to whoever owns the extension:

- An extension goes to a format contributed to `model.formats`. If two plugins
  claim the same extension, the one with the lower plugin id wins.
- A format that fails on a file (it throws, or returns `{ rejected }`) skips only
  that file. The import summary names the file and the reason, and the rest
  continue.

Categories, detail-level grouping, skipping duplicate names and committing to
the library are the same for every format, because they happen after the file
has become glTF.

Plugins never import each other's code at run time. A format plugin returns its
contributions from its entry module and the app carries them to the panel. The
types of the contract live in **Models** (`dist/api.d.ts`) and the other plugins
import them type-only, through a `paths` entry in their `tsconfig.json`.

### The `model.api` service

Animation clips are pulled out of model files, so **Animations** needs what
**Models** does: scan a folder for the files the formats on now can read, and
convert a file into glTF. It gets that from **Models** as a service. The entry
module of **Models** contributes one value to its own extension point
`model.api`, built inside its `definePlugin(ctx)` function so that every call
runs with the file permissions of **Models**; the panel of **Animations** reads
it with `ctx.extensions(...)`. **Animations** itself asks for no file
permissions and has no entry module.

```ts
export const MODEL_API = "model.api";
export interface ModelFile { relPath: string; stem: string; ext: string }
export interface ModelApi {
  formats(): { id: string; label: string; extensions: string[] }[];
  lookingFor(): string;   // "Looks for .gltf and .glb files." or the no-formats sentence
  scan(sourceRoot: string): Promise<ModelFile[]>;
  convert(sourceRoot: string, libraryRoot: string, file: ModelFile, pack: string, category: string):
    Promise<{ file: string; warnings: string[] } | { rejected: string }>;
  hasAnimationClips(libraryRoot: string, pack: string, category: string, file: string): Promise<boolean>;
}
```

The service reads the formats on every call, so it follows format plugins being
turned on and off. `convert` writes the loose glTF under
`library/<pack>/<category>/` and keeps no original. The animation panel then
commits `animation` entries; the host extracts the clips from them. An animation
import keeps **no original files** and attaches no `origin`: the source is
replaced by the extracted clips, and the host rejects kept files on an animation
source. If **Models** is off there is no service, and the panel says so (the
plugin cannot be on without it anyway).

## The contract

```ts
export const MODEL_FORMATS = "model.formats";

export interface ModelSource { root: string; path: string; stem: string }
export interface ModelTarget { libraryRoot: string; pack: string; category: string }
export type ModelImportResult =
  | { file: string; warnings: string[]; originals?: string[] }
  | { rejected: string };
export interface ModelFormat {
  id: string;            // "fbx"
  label: string;         // "FBX"
  extensions: string[];  // lower case, no dot
  keepsOriginal?: boolean; // default true; false when the stored file is the original
  import(source: ModelSource, target: ModelTarget): Promise<ModelImportResult>;
}
```

`import` converts one source file into the library's model form: a loose glTF
at `library/<pack>/<category>/<stem>.gltf` under `target.libraryRoot`, whose
single buffer is `<stem>.bin` beside it, with its texture images beside it too.
It returns the `.gltf` file name. Read the source through the plugin's own file
access (`ctx.fs.readBytes(source.root, source.path)`) and write with
`ctx.fs.writeText` / `writeBytes` against `target.libraryRoot`.

### glTF models

`com.toybox.model-gltf` claims `gltf` and `glb`. A `.gltf` is copied with its
buffer renamed to `<stem>.bin` and its relative textures beside it; a `.glb` is
split into the same shape. Files outside the library's shape are skipped with a
short reason: `multi-buffer` (more than one buffer), `bad-gltf`, `not-a-glb`,
`unsupported-version`, `no-bin-chunk`, `embedded-images` (images inside a GLB's
binary chunk). It sets `keepsOriginal: false`: the stored file already is the
original, so nothing else is kept. The entry's `origin` names only the plugin
(`{ "plugin": "com.toybox.model-gltf" }`), so turning the plugin off hides what
it imported. Models imported before the split record `com.toybox.model-core` and
stay visible with **Models**.

### Keeping the original

Converting can lose things, so for every model that a contributed format
converts, **Models** keeps the source file itself beside the converted glTF, the
way a Substance material keeps its `.sbsar`. A format that needs more than the
one file to read it again (an OBJ's material files) returns them in `originals`:
paths relative to `source.root`, only files the import really read. Do not list
the source itself or the images the converter already copied beside the glTF.
A format with `keepsOriginal: false` keeps nothing.

The entry is committed with:

```json
"format": "obj",
"origin": {
  "plugin": "com.example.model-obj",
  "files": ["crate.obj", "crate.mtl"],
  "meta": { "format": "obj", "source": "props/crate.obj", "companions": { "crate.mtl": "crate.mtl" } }
}
```

- `plugin` is the plugin that contributed the format, not **Models**. The host
  accepts that name only from an importer for a plugin that is on and directly
  depends on it; any other claim is replaced by the importer's own id. Turning
  the contributing plugin off therefore hides what it imported, like any
  plugin's items.
- `files[0]` is the source file, the rest are its companions, all bare names.
  They are stored once per library, by content
  (`library/models/_sources/<sha256>/<name>`), so a companion several models share
  is one file; they travel with the model and are deleted with the last model that
  keeps them. They are not exported by Copy files. `asset.origin.files` gives plugins
  their library-relative paths and `asset.format` the format.
- `meta.source` is the path under the folder that was imported;
  `meta.companions` maps each companion's stored name to where it lay relative
  to the source file's folder (a companion above it is recorded by name only).
- A name never collides: an original does not take the name of a converted
  file (glTF, bin, images), and two different companions with the same name are
  stored as `name.mtl` and `name_2.mtl`. A companion shared by several models
  is stored once per model folder. A file that cannot be copied is left out with
  a warning; if the source itself cannot be copied the model has no origin.
- Each detail level (`_LOD1`, ...) keeps its own original in its own folder.
- Animation clips extracted from a model record the same `plugin` and `meta`
  and `format` but no `files`, so the original is stored once, with the model.

The asset properties show an **Imported from** row: the format, with
"(original kept)" when it has origin files.

## Adding another format

Make a plugin folder with a manifest that depends on **Models**:

```json
{
  "id": "com.example.model-stl",
  "name": "STL models",
  "version": "1.0.0",
  "dependencies": { "com.toybox.model-core": "^3.0.0" },
  "assetKinds": ["model"],
  "entry": "dist/index.js",
  "permissions": { "fsRead": true, "fsWrite": true }
}
```

and an entry module that contributes the format:

```ts
import { definePlugin } from "@ldlework/toybox-sdk";
import type { ModelFormat } from "com.toybox.model-core"; // type-only, see tsconfig "paths"

export default definePlugin((ctx) => {
  const stl: ModelFormat = {
    id: "stl",
    label: "STL",
    extensions: ["stl"],
    async import(source, target) {
      const bytes = await ctx.fs.readBytes(source.root, source.path);
      const converted = toGltf(bytes, source.stem); // your code: { gltf, bin }
      if (!converted) return { rejected: "The file has no triangles." };
      const dir = `library/${target.pack}/${target.category}`;
      await ctx.fs.writeText(target.libraryRoot, `${dir}/${source.stem}.gltf`, JSON.stringify(converted.gltf));
      await ctx.fs.writeBytes(target.libraryRoot, `${dir}/${source.stem}.bin`, converted.bin);
      return { file: `${source.stem}.gltf`, warnings: [] };
    },
  };
  return { extensions: { "model.formats": [stl] } };
});
```

The same module can also export models by adding `run` and `preview` next to
`extensions`. Point the type import at the built declarations with a `paths`
entry in the plugin's `tsconfig.json`:

```json
"paths": { "com.toybox.model-core": ["../com.toybox.model-core/dist/api.d.ts"] }
```

The point name `"model.formats"` is repeated as a plain string; there is no run
time import. Build **Models** first, then the format plugin.

A format that needs the FBX2glTF converter calls `ctx.host.convertToGltf`
(permission `rustConvert`); the converter's path is the app preference shown
under **Settings → Plugins** for every plugin with that permission.

## The GLB exporter

The **Merged GLB files** exporter (and **Copy files**) are built into the app,
not part of any plugin. Moving GLB export into **glTF models** would mean a plugin
exporter that re-implements the built-in one's file naming, name-collision,
variant-policy and report handling on top of `assembleGlb`, and it also serves
animations, which belong to a different plugin. That is not a small change, so
the exporter stays in the host and **glTF models** owns the import only.

## Planned: format loaders

Not implemented. The agreed direction: a format plugin will be able to supply
its own loader for the viewer, so a model is displayed straight from its kept
original, and glTF is produced only when a feature needs it (export, rig
matching, animation extraction). The groundwork is already in place:
`origin` keeps the original file and its companions with the model, and
`format` records what it was imported from. Until then every model is converted
to glTF at import and shown from that.
