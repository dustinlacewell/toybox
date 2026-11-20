# Asset types

Every section of the library — Models, Materials, Animations, Audio, Skyboxes —
is an **asset type**, and a type exists because a plugin declares it. A plugin
that lists `assetTypes` in its manifest owns one or more types end to end: the
declaration, the importer and, for a type the plugin implements itself, the
viewer, the thumbnails and the property rows. Adding a new kind of resource to
Toybox is writing one plugin; the app is not changed. How a plugin is put
together in general (what it ships, dependencies, extension points) is in
[Plugins](plugins.md).

The bundled [Skyboxes plugin](skyboxes.md) is the reference implementation of a
plugin-defined type. The bundled [Substance materials plugin](substance-materials.md)
shows the other shape: a plugin that adds to a type another plugin declares.

## Two kinds of implementation

| `implementation` | Who views and indexes it | Asset shape | Used by |
| --- | --- | --- | --- |
| `plugin` (default) | The plugin's entry module | Generic: `files` + `meta` | Skyboxes, any new type |
| `native` | The host | The host's own (`fileset`, `material`, `audio`, …) | Models, animations, materials, audio |

The four native types are declared by their bundled plugins like any other
(Models by `com.toybox.model-core`, Animations by `com.toybox.animation-core`,
Materials by `com.toybox.material-core`, Audio by `com.toybox.audio-core`; the
plugin-implemented Skyboxes by `com.toybox.skybox-core`; see [Models](models.md)
and the table in [Plugins](plugins.md#the-bundled-plugins). Each type has one
`<type>-core` plugin that declares it and owns its import panel; a plugin that
adds to a type, such as `com.toybox.material-sbsar`, depends on that core) — the declaration supplies the label, icon,
navigation order and importer — but
their viewers and indexing stay in the host, because they rest on native code:
glTF parsing, rig matching and animation extraction, audio decoding. The host
also keeps a built-in declaration for each, so the library still works when one
of those plugins is not installed. Moving a native type's viewer into its plugin is
possible type by type; nothing in the contract prevents it.

## Declaring a type

`manifest.json`:

```json
{
  "id": "com.example.skies",
  "name": "Skyboxes",
  "version": "1.0.0",
  "entry": "dist/index.js",
  "assetTypes": [
    { "id": "skybox", "label": "Skyboxes", "singular": "Skybox", "icon": "icons/skybox.svg", "order": 50 }
  ],
  "permissions": { "fsRead": true, "fsWrite": true },
  "ui": { "importPanel": "dist/ui/Import.js" }
}
```

- `id` — lower-case letters, digits and dashes, 2–32 characters. It also names
  the type's storage folder, so the host's own names (`model`, `models`,
  `audio`, `plugin`, …) are refused.
- `label` / `singular` — navigation label and noun.
- `icon` — plugin-relative artwork for the navigation.
- `order` — navigation position; the native types use 10–40.
- `properties` — numbers of the type's assets that the library can sort by and
  filter on with a range (see below).
- `tileAspectRatio` — width ÷ height of library tiles (default 1).
- `implementation` — `"native"` only for a built-in kind.

A plugin works with the types it declares unless `assetKinds` says otherwise.
If two plugins declare one id, the first by plugin id owns it and the other is
reported as a plugin error.

## Properties: sorting and range filters

A type lists the numbers of its assets that are worth sorting by or narrowing
to a range. The library reads each one straight from the stored asset record,
so a property is a path, not code, and works in the app, over a remote library
and for items whose plugin is not loaded:

```json
"assetTypes": [
  { "id": "skybox", "label": "Skyboxes",
    "properties": [
      { "id": "width", "label": "Width", "path": "meta.width", "unit": "px" },
      { "id": "exposure", "label": "Exposure", "path": "meta.exposure", "use": ["range"] }
    ] }
]
```

- `id` — lower-case letters, digits and dashes, unique within the type.
- `label` — shown in the sort menu and the filter pane; `unit` is written after
  the numbers.
- `path` — dotted path to a number in the asset record: `meta.<field>` for a
  plugin type (whatever the importer put in `meta`), the host's own fields for a
  native type (`audio.duration`, `clip.duration`, `animation.clipCount`). Words
  of letters, digits and `_`; anything else is reported as a plugin problem and
  left out.
- `use` — `sort`, `range` or both (the default).

Sorting by a property puts the items without it last; a range leaves them out.
A native type redeclared without `properties` keeps the host's list (Models:
animation clips; Animations: length; Audio: duration, sample rate, channels).
The sort and the ranges are part of the list query (`sort.key` is
`prop:<path>`, `ranges` holds `{ path, min, max }`), so a plugin type gets them
with no code of its own.

## Implementing a type

The entry module provides one implementation per plugin-implemented type, under
`types` of the module it default-exports (`defineAssetTypes` builds that
shape; `definePlugin` takes the same `types` next to any other member). Every
member is optional: a type with none still imports, lists, filters, tags,
collects, deletes and exports. A plugin that declares a plugin-implemented type
but provides no `types` is reported as a plugin error.

```ts
import { defineAssetTypes } from "@ldlework/toybox-sdk";

export default defineAssetTypes({
  types: {
    skybox: {
      Viewer,                       // full-view stage; receives { ctx }
      renderThumbnail,              // (asset, files, size) => PNG bytes
      properties: (asset) => [{ label: "Layout", value: "Panorama" }],
      subtitle: (asset) => "Panorama · HDR",
    },
  },
});
```

- **`Viewer`** fills the stage of the host's viewer frame. The frame keeps the
  title, details, favourite and export controls. `ctx.setSidebar` and
  `ctx.setFooter` place the type's own controls in the frame.
- **`renderThumbnail`** returns a square PNG. The host queues, stores and
  refreshes thumbnails; it waits for plugin discovery before asking.
- **`files`** (`ctx.files`, and the second argument of `renderThumbnail`) reads
  the asset's own files by name: `fileUrl(asset, name)` for an image or media
  source, `readFile(asset, name)` for bytes. A plugin cannot name a path
  outside the asset it was given.

Plugin modules resolve `react`, `three`, `three/addons/loaders/HDRLoader.js`,
`three/addons/loaders/EXRLoader.js`, `lucide-react`, `@ldlework/toybox-sdk` and
`@ldlework/toybox-sdk/ui` to the host's own copies through the import map, so
they render with the same React and design system and need no bundler. To
share another module with plugins, add it to the import map in `vite.config.ts`.

## Assets of a plugin type

An importer commits `PluginSeedEntry` values:

```json
{ "kind": "plugin", "type": "skybox", "id": "uid://skybox_1f…", "pack": "_import_9c…",
  "category": "skies", "name": "Sunset", "files": ["sunset.hdr"],
  "meta": { "layout": "equirect", "encoding": "hdr", "format": "HDR", "width": 8192, "height": 4096 } }
```

- `files` are bare names, unique within the asset. Before commit they sit at
  `library/<pack>/<type>/<name>/<file>`; use `ctx.fs.copyFile` to put them there
  without passing large files through the plugin.
- `meta` is the type's own record. The host stores it and hands it back; only
  the plugin interprets it. Read it defensively — it lives in a user-editable
  manifest.
- On commit the host moves the files into managed storage,
  `library/<type>/<key>/<file>`, and writes that folder as `storagePath`. File
  names are kept. Indexing checks every file exists.

In the catalog the asset is `{ kind: "plugin", type, files, meta, … }` with
library-relative `files`. Plugins receive it as `PluginAssetView`.
`assetTypeOf(asset)` gives the library type of any asset.

## What the host does for every type

Every seed entry (model, material, animation, audio and plugin types) may carry
`format`: the file format the asset was imported from, a short lower-case id
without a dot (`"fbx"`, `"obj"`, `"glb"`, `"sbsar"`, `"wav"`). The host trims, strips
leading dots and lower-cases it, stores it in the manifest entry and exposes it as
`asset.format`; it drives the Formats filter. Without it the filter falls back to the
extension of the asset's primary file (models are stored as glTF, so `gltf`).

The same entries may carry `tags`: descriptive keywords that belong to the asset and
live in the library, so they travel with it to a [server](server.md) and are seen by
everyone who has the library (`["footstep", "carpet", "loop"]`; see
[tags](libraries.md#tags)). Optional, on the entry
and on each of its variants (a variant has its own; it does not inherit its parent's).
The host normalises them: trimmed, inner whitespace collapsed, lower-cased, empties and
duplicates dropped, order kept; a tag with a colon is dropped (so it cannot pose as the
`source:` / `publisher:` / `license:` tags derived from `provenance`); at most 64 tags
of 48 characters, the rest is cut without an error. They reach the catalog as
`asset.tags` (not inside `asset.user`) and are searchable and filterable next to the
provenance-derived tags of `asset.user.tags`. Importing the same id again without `tags` keeps the ones the
library already has for it. They are not part of an item's content fingerprint or of the
key its files are stored under. An animation file imported with tags gives them to the
clips cut from it; a model's tags are not copied to the clips extracted from it.
SDK type: `SeedEntryInput` has `tags?: string[]`.

Navigation and counts, search, category / source / publisher / license / format / tag
filters, favourites, collections, provenance on import, selection, deletion
(files and thumbnail), saved exports with change tracking, and the **Copy
files** exporter: one file is exported under the item's name; a set keeps its
own file names in a folder named for the item.

Assets of a type whose plugin is not installed are kept: they get a section,
their details and files stay available and they still export. Only the viewer
and thumbnail are missing until the plugin returns.

## Turning plugins on and off

**Settings → Plugins** lists every installed plugin as a row with a checkbox;
the search box and **Enabled only** narrow the list, and a row opens to the
plugin's details and preferences. A plugin that is off is not loaded: its
importer, exporter, asset types and rig profiles are gone until it is turned
back on, which takes effect at once. A plugin that needs another plugin
([dependencies](plugins.md#dependencies)) is off while that one is off: turning
one on turns on what it needs, and a plugin others need cannot be turned off
until they are.

Turning a plugin off hides what it brought into the library. Several plugins
can put items in one type, so hiding follows the plugin an item came from:

- an item imported by a plugin that is off is hidden;
- an item with no recorded plugin belongs to the plugin that declares its type,
  and is hidden when that one is off;
- an item that records a plugin id which has been retired follows the plugins
  that replaced it, and is hidden only when none of them is on (see
  [Retired plugins](plugins.md#retired-plugins));
- a type's section stays while a plugin that is on declares it or, for a type
  the host implements, still imports into it.

Hidden items leave counts, filters, collections and search. Nothing in the
library is deleted or rewritten — the files, manifest entries and thumbnails
stay, and everything returns when the plugin is turned back on. The choice is
stored as `disabledPlugins` in the app's `settings.json`; a plugin not listed
there is on.

Preferences the host keeps appear under the plugin they belong to: the FBX
converter path under every plugin allowed to convert files (`permissions.rustConvert`;
none of the bundled ones today),
the material preview shape under the plugin that declares materials.

## Several plugins, one type

A type has one declaring plugin, but any number of plugins can import into it.
A plugin that ships an `ui.importPanel` is an importer. It lists the types it
produces in `assetKinds` (the types it declares are included); the import
screen offers every importer for the current type.
The bundled [Substance materials](substance-materials.md) plugin imports
`material` assets alongside the texture-folder importer.

Every entry an import commits is stamped with its plugin:

```json
"origin": { "plugin": "com.toybox.material-sbsar",
            "meta": { "sbsar": "tiles.sbsar", "preset": "Green", "inputs": { "joint_height": 0.3 }, "size": 11 } }
```

The host writes `plugin` itself, so a plugin cannot claim another's items. An
importer may supply `origin.meta`: its own record of how the item was made,
stored and handed back untouched as `asset.origin.meta`.

A material may also keep **source files** — what it was generated from. The
importer stages them next to the maps and lists their bare names in
`origin.files`. At commit the host moves each into shared storage, by content
(`library/materials/_sources/<sha256>/<name>`: one copy per distinct file in the
library, however many materials keep it; a staged copy of a file that is stored
already is dropped), and checks they exist when indexing. They go with the last
material that keeps them and are not exported. A plugin reads them back as
library-relative paths in `asset.origin.files`; the last segment is the file's
original name, so find a file by name, not by where it sits. A model keeps them the
same way (names other than `<name>.gltf` and `<name>.bin`; see
[Models](models.md#keeping-the-original)); it is not an error if one goes
missing. Other types cannot keep source files today. An importer may also name,
in `origin.plugin`, a plugin that is on and depends on it; the host stamps its
own id otherwise. The origin decides
what [turning a plugin off](#turning-plugins-on-and-off) hides, and which
plugin's panel an item's viewer shows.

### A panel in the viewer

`ui.assetPanel` names a module that default-exports a component
(`defineAssetPanel`). The viewer shows it as a tab — labelled
`ui.assetPanelLabel`, else the plugin's name — for every item whose
`origin.plugin` is that plugin. Its `ctx` carries the asset, the jailed `fs`,
`services`, the host widgets, and `commit(entries)`, which adds entries exactly
as an import does. An entry that reuses the asset's `id` replaces that item in
place and gets a new thumbnail; another id adds a new item. When a material or
a plugin-defined item is replaced, the folder it used to occupy is removed.

## Plugins that drive an external program

A plugin that needs a program on the user's machine — a renderer, a converter —
can do all of that work itself through `ctx.services` (import panels and asset
panels both have it).

**Settings.** A manifest declares what it needs from the user:

```json
"settings": [
  { "key": "engine", "type": "file", "label": "Substance engine",
    "default": "{appData}/Adobe/Substance3DIntegrationTools/substance_remote_engine.exe",
    "hint": "Where to find it." },
  { "key": "size", "type": "select", "label": "Default texture size", "default": "11",
    "options": [{ "value": "10", "label": "1024" }, { "value": "11", "label": "2048" }] }
]
```

Types are `text`, `checkbox`, `select`, `directory` and `file`. They appear
under the plugin's **Preferences** in Settings → Plugins and are stored per
plugin as `pluginSettings` in `settings.json`. A text default may use
`{appData}`, `{localAppData}` and `{home}`. `services.settings.get()` returns
what the user chose, else each default; `set(key, value)` stores one.

**Tools** (`permissions.runTools`). `services.tools.launch(setting, args)`
starts the program a `file` setting points at, in that program's folder. The
path always comes from the setting — a plugin cannot name a program in the
call. The program runs in the background and is stopped when Toybox closes.
`services.tools.run(setting, args, { timeoutMs? })` runs it to completion and
returns `{ code, stdout, stderr }`; it rejects when the program cannot start or
outlives the timeout (two minutes by default), and a non-zero exit code is
returned, not thrown. `hostProcessId()` gives Toybox's process id, for a program
that tracks who it works for.

**Local requests** (`permissions.localNetwork`). `services.local.isOpen(port)`
and `requestJson(port, message, timeoutMs?)` talk to a service on this machine
— `127.0.0.1` only — one JSON message and its JSON reply per call.

**Scratch.** `services.scratchDir(clear?)` is the plugin's private folder for
intermediate files, as an absolute path usable with `ctx.fs`. Nothing in it is
kept.

## Compatibility

A library that contains plugin-defined assets needs a Toybox build that knows
the generic `plugin` kind. Releases up to 0.4.1 reject such a library, and do
not load plugins that declare types. Try a development build against a separate
library and app-data folder — see [Running an isolated build](#running-an-isolated-build).

## Running an isolated build

A development build shares the installed app's identifier, so it would upgrade
the bundled plugins in the installed app's data folder to manifests an older
release cannot read. Run it under its own identifier instead:

```
pnpm tauri dev --config src-tauri/tauri.isolated.conf.json
```

It starts with no library; create or pick one that the installed app does not
use.
