# Plugins

A plugin is a folder with a `manifest.json` and the files the manifest points
at. Plugins have no kind. What a plugin does follows from what it ships, and one
plugin can do several things at once.

| The plugin ships | So it |
| --- | --- |
| `assetTypes` in the manifest | declares library types (see [Asset types](asset-types.md)) |
| `ui.importPanel` | is an importer, offered on the import screen for the types in `assetKinds` |
| an entry module whose export has `run` | is an exporter, offered for the types in `assetKinds` |
| an entry module whose export has `profiles` | adds rig profiles (see [Rig adapters](rig-adapters.md)) |
| an entry module whose export has `types` | implements the plugin-implemented types it declares |
| an entry module whose export has `extensions` | contributes to extension points of other plugins |
| `ui.exportPanel`, `ui.assetPanel`, `fields`, `settings` | adds settings and panels to the above |

A `kind` field in an older manifest is ignored.

## The bundled plugins

Every plugin has one responsibility, and each library type has one
`<type>-core` plugin that declares it and owns its import panel. Plugins that
add to a type depend on its core and never import each other: they meet through
manifests and extension points.

| Plugin | Id | Version | Needs | Its one responsibility |
| --- | --- | --- | --- | --- |
| **Models** | `com.toybox.model-core` | 3.0.0 | | The Models type and its importer; publishes `model.formats` and `model.api`. |
| **glTF models** | `com.toybox.model-gltf` | 1.0.0 | model-core `^3.0.0` | The glTF and GLB model format. |
| **Animations** | `com.toybox.animation-core` | 1.0.0 | model-core `^3.0.0` | The Animations type and its importer. |
| **Materials** | `com.toybox.material-core` | 2.0.0 | | The Materials type and the texture-folder importer. |
| **Audio** | `com.toybox.audio-core` | 2.0.0 | | The Audio type and the audio file importer. |
| **Skyboxes** | `com.toybox.skybox-core` | 2.0.0 | | The Skyboxes type and its importer. |
| **Synty Polygon rig** | `com.toybox.rig-synty-polygon` | 1.0.0 | model-core `^3.0.0` | The Synty Polygon bone names. |
| **Synty Sidekick rig** | `com.toybox.rig-synty-sidekick` | 1.0.0 | model-core `^3.0.0` | The Synty Sidekick bone names. |
| **Synty Unreal rig** | `com.toybox.rig-synty-unreal` | 1.0.0 | model-core `^3.0.0` | The Unreal-skeleton bone names used by Synty. |
| **Quaternius Humanoid rig** | `com.toybox.rig-quaternius` | 1.0.0 | model-core `^3.0.0` | The Quaternius humanoid bone names. |
| **Placer** | `com.toybox.placer` | | | Exports models to a game project. A development plugin: it is not bundled with the app. |

The eleven bundled plugins are listed in `BUNDLED_PLUGINS`
(`src-tauri/src/infra/bundled.rs`), shipped by `bundle.resources` in
`tauri.conf.json` and built, cores first, by `plugins:build`. A test checks that
each manifest id is its folder name and that every dependency is bundled at a
version that satisfies it.

## The manifest

```json
{
  "id": "com.example.obj",
  "name": "OBJ models",
  "version": "1.0.0",
  "entry": "dist/index.js",
  "dependencies": { "com.example.models": "^2.0.0" },
  "assetKinds": ["model"],
  "description": "Imports .obj files as models.",
  "permissions": { "fsRead": true, "fsWrite": true }
}
```

- `id`, `name`, `version`, `description`, `icon`, `assetIcons` identify the plugin.
- `entry` is the plugin-relative path of the entry module. It is optional: a
  plugin that only declares native types and ships an import panel needs none.
- `assetKinds` lists the types the plugin works with. A plugin works with the
  types it declares in addition. With neither, it works with models.
- `permissions` are `fsRead`, `fsWrite`, `rustConvert`, `rustExport`,
  `runTools` and `localNetwork`. A method the manifest did not ask for throws
  when called.
- `ui`, `fields` and `settings` are the panels, the declarative export fields
  and the user preferences, described in [Asset types](asset-types.md).

## Dependencies

`dependencies` maps a plugin id to the version range it accepts (semver, `*`
for any). A plugin can load only when everything it lists is installed, at an
accepted version, switched on and itself able to load.

- Turning a plugin on also turns on the plugins it needs. Settings → Plugins
  says which ones changed.
- A plugin that other switched-on plugins need cannot be turned off. Its row
  says who needs it, and the host refuses the request with the same sentence if
  it is made anyway. Turn the dependents off first.
- A plugin the user switched on but whose dependencies are not usable is
  blocked. Nothing from it is loaded and its row says why, for example
  `Needs Models, which is turned off.` or `Needs Models 2.0.0 or compatible
  (^2.0.0); version 1.1.0 is installed.`
- Two plugins that need each other are both blocked.

Anything that is switched off or blocked is not active. Only active plugins
contribute types, importers, exporters, rig profiles and extensions, and the
items of a type whose plugins are not active are hidden, not deleted.

## The entry module

The entry module default-exports a `PluginModule`, or a function that takes the
plugin's `PluginCtx` and returns one (it may be async). `definePlugin` types
either form.

```ts
import { definePlugin, type PluginCtx } from "@ldlework/toybox-sdk";

export default definePlugin((ctx: PluginCtx) => ({
  run: async (exportCtx, assets) => { /* write files, report them */ return exportCtx.report.done(); },
  preview: async (exportCtx, assets) => [],
  profiles: [],
  types: {},
  extensions: {},
}));
```

Every member is optional and they combine freely; a module may provide nothing.
A default export that is neither an object nor a function, an import that
fails, or an activation that throws becomes an error entry for that plugin.
The other plugins load as usual.

With several libraries (see [Libraries](libraries.md)) an `AssetView` carries a qualified, opaque `id` and a `library` field. Pass ids back to the host unchanged. `host.getLibraryRoot()` is the root of the library the panel works in: the library an import is going into, or the library of an asset panel's asset. An asset panel that commits an entry with `asset.id` replaces that item in its own library.

`PluginCtx` is `{ id, host, fs, services, extensions }`, limited to that
plugin's permissions. A function the module hands out, such as an extension
that another plugin calls later, keeps working with the plugin's own
permissions and settings.

The export, import, viewer-panel and asset-panel contexts also carry
`services` and `extensions`.

## Extension points

An extension point is a contract one plugin publishes for others. The host does
not interpret it: it only carries the contributions.

A core plugin names the point (by convention `<subject>.<what>`, for example
`model.formats`) and documents what one contribution looks like. Plugins that
depend on it contribute values under that name:

```ts
// com.example.obj, which depends on com.example.models
export default definePlugin((ctx) => ({
  extensions: {
    "model.formats": [{ extension: ".obj", label: "Wavefront OBJ", toGltf: (file) => convert(ctx, file) }],
  },
}));
```

The core plugin reads what is installed and active whenever it needs it:

```ts
const formats = ctx.extensions<ModelFormat>("model.formats");
// [{ pluginId: "com.example.obj", value: { extension: ".obj", ... } }, ...]
```

- Contributions are ordered by plugin id, then by their order in the array.
- `ctx.extensions` is looked up on each call, so a lookup a plugin kept always
  sees the plugins that are on now. Turning a plugin off removes its
  contributions the next time the plugins load again.
- `extensions[point]` must be an array. Any other shape is an error for the
  contributing plugin and leaves the rest alone.

The bundled model plugins are the worked example: `com.toybox.model-core`
publishes `model.formats`, `com.toybox.model-gltf` depends on it and
contributes the glTF format, and `com.toybox.animation-core` depends on it too.

A plugin can also contribute to its own point. `com.toybox.model-core` returns,
under `model.api`, a small service object built inside its `definePlugin`
function; the import panel of `com.toybox.animation-core`, a plugin with no
entry module, reads it with `ctx.extensions("model.api")`. This is how a plugin
lends a capability to the plugins that need it without anyone importing anyone:
the service runs with its own plugin's permissions. The host keeps one table of
every active plugin's contributions, so it reaches panels as well as entry
modules, whoever contributed it. They are described in [Models](models.md).

## Services

`ctx.services` is the same on every context.

- `settings.get()` and `set(key, value)` read and store the plugin's declared
  settings.
- `tools.launch(setting, args)` starts, in the background, the program a `file`
  setting points at. `tools.run(setting, args, { timeoutMs? })` runs it to
  completion (two minutes by default) and returns `{ code, stdout, stderr }`. It
  rejects when the program cannot start or outlives the timeout, and a non-zero
  exit code is returned, not thrown. The path always comes from the setting; a
  plugin cannot name a program. `tools.hostProcessId()` returns Toybox's own
  process id. These need `permissions.runTools`.
- `local.isOpen(port)` and `local.requestJson(port, message, timeoutMs?)` talk
  to a service on this machine (`127.0.0.1` only). They need
  `permissions.localNetwork`.
- `scratchDir(clear?)` is the plugin's private folder for intermediate files.

The built-in exporters (merged GLB, copy files) have no plugin behind them, so
their services have no permissions.

## Retired plugins

When a bundled plugin is split, renamed or dropped, its id goes into the
retirement table (`RETIRED_PLUGINS` in `src-tauri/src/infra/bundled.rs`): the old
id and zero or more replacement ids. Adding a line is all it takes to retire
another one.

- At start-up an unmodified managed copy of the old plugin moves to
  `plugin-backups/`. A linked or edited copy is left alone.
- The old id is not listed while every replacement is installed. A plugin
  retired with no replacement is never listed.
- If the user had the old plugin switched off, its replacements start switched
  off too.
- The preferences the user stored for the old plugin (Settings → Plugins, or
  `services.settings`) are kept in `settings.json` under the plugin id. At
  start-up they move to each replacement, once: a value the replacement already
  has wins, and the old entry is removed.
- A saved export that names a retired exporter follows the one plugin that
  replaced it. When several replaced it, the export shows "Plugin unavailable"
  until the user chooses one.
- Library items keep the plugin id they were imported under. Each replacement's
  manifest is delivered to the app with `replaces`, the retired ids it takes over,
  and an item that records a retired id in `origin.plugin` follows its
  replacements: it is hidden only when none of them is on, and its viewer panel
  is the panel of the replacement that ships one. A retired id with no
  replacement behaves like any plugin that is not installed: the item belongs to
  the plugin that declares its type. The library manifest is never rewritten.

| Retired | Replaced by |
| --- | --- |
| `com.toybox.gltf-importer` | `com.toybox.model-core`, `com.toybox.model-gltf`, `com.toybox.animation-core` |
| `com.toybox.material-importer` | `com.toybox.material-core` |
| `com.toybox.audio-importer` | `com.toybox.audio-core` |
| `com.toybox.skybox` | `com.toybox.skybox-core` |
| `com.toybox.humanoid-rigs` | `com.toybox.rig-synty-polygon`, `com.toybox.rig-synty-sidekick`, `com.toybox.rig-synty-unreal`, `com.toybox.rig-quaternius` |
| `com.toybox.model-fbx` | nothing |
| `com.toybox.model-obj` | nothing |

What is keyed by a plugin id, and how a retired id is followed:

| Keyed by plugin id | How it follows a retired id |
| --- | --- |
| The switch (`disabledPlugins`) | Copied to the replacements at start-up. |
| Preferences (`pluginSettings`) | Moved to the replacements at start-up. |
| `origin.plugin` of library entries | Never rewritten; resolved through `replaces` whenever visibility or the viewer panel is decided. |
| Who owns a plugin-declared type (such as `skybox`) | The replacement declares the type; entries with no recorded plugin belong to it. |
| Saved exports (`exporterId`) | Resolved through `replaces` when the export is opened; the file is rewritten only when the user saves it. |
| Scratch folders (`plugin-scratch/<id>`) | Not carried over: the folder is intermediate files that the plugin clears and recreates. |
| Type implementations, import and viewer panels, thumbnails | Keyed by type id, asset id or the loaded manifest, not by a stored plugin id. |

## In Settings

**Settings → Plugins** shows each plugin as a row with a checkbox. The line on
the right says what it provides, in words: the labels of the types it declares,
then what else applies (`Imports materials`, `Exporter`, `Rig profiles`,
`Extends Models`). The details list adds `Requires` and `Required by` when they
apply. The FBX converter preference shows under every plugin that has
`permissions.rustConvert` (none of the bundled plugins today); the material preview shape shows under the plugin
that declares materials.
