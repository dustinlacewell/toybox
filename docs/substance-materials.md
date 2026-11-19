# Substance materials (.sbsar)

The bundled **Substance materials** plugin (`com.toybox.material-sbsar`, which
depends on **Materials**, `com.toybox.material-core`) turns
Substance archives into ordinary library materials. An `.sbsar` is a procedure,
not a set of images: the plugin has Adobe's Substance engine render it into
colour, normal, roughness, metal, shadowing and height maps, with the preset
and parameter values you choose.

The results are listed under **Materials** next to materials from texture
folders. They preview, tag, collect and export exactly the same.

## What you need

The plugin drives the **Substance Remote Engine**, the
`substance_remote_engine` program in Adobe's *Substance 3D Integration Tools*.
Adobe's Substance add-ons for Blender, Unity, Unreal and other tools install
it, normally at:

| System | Location |
| --- | --- |
| Windows | `%APPDATA%\Adobe\Substance3DIntegrationTools\` |
| macOS | `~/Library/Application Support/Adobe/Substance3DIntegrationTools/` |
| Linux | `~/.Adobe/Substance3DIntegrationTools/` |

Toybox does not ship or download the engine. **Settings → Plugins → Substance
materials → Preferences** holds its location:

- **Substance engine** — the program file. It defaults to the Windows location
  above; choose the file yourself anywhere else.
- **Render with** — *Automatic* uses the graphics card. Vulkan and *Processor
  only* are there for machines where that fails.
- **Default texture size** — the size the import screen starts at.

## Importing

**Import → Substance materials (.sbsar)**, then choose a folder. Every `.sbsar`
in it is listed and ticked. Selecting one opens it in the engine and shows:

- a live preview, rendered at 512 pixels and redrawn as you change things;
- **Preset** — the presets the archive's author shipped;
- the archive's parameters, each with the control its author gave it (slider,
  colour, switch or list), in the archive's own groups;
- **Import all presets as separate materials** — one material per preset,
  named `<archive> - <preset>`, each as its author made it.

**Texture size** on the left applies to the import. Archives you never
selected are imported at their defaults.

## Where the archive lives

A Substance material is a material folder with its maps, and the archive it was
rendered from, which the library stores **once**, however many materials (presets) were
made from it:

```
library/materials/<key>/
  albedo.png  normal.png  roughness.png  metallic.png  ao.png  height.png
library/materials/_sources/<sha256 of the archive>/
  ceramic_tiles_blue.sbsar
```

The maps are what the library shows, previews and exports. The archive is the
source they were rendered from; the material's entry records it, together with
the preset and the parameters that were changed. The plugin stages the archive
next to the maps as it always did and lists its bare name; when the material is
committed the library moves it into `_sources/` (or drops the staged copy when
the same archive is stored already) and the stored entry lists that path:

```json
"origin": { "plugin": "com.toybox.material-sbsar",
            "files": ["library/materials/_sources/4f0c…/ceramic_tiles_blue.sbsar"],
            "meta": { "sbsar": "ceramic_tiles_blue.sbsar", "graph": 0, "preset": "Commander Green",
                      "inputs": { "joint_height": 0.3 }, "size": 11 } }
```

So the material can be rendered again after the original download is gone, and
it stays whole when a library is copied or moved: a copy to another library
carries the archive once for all the materials that share it, and none at all
when that library has it. The archive is deleted with the last material that
keeps it and is not part of an export — a game project gets the textures. Ten
presets of a 200 MB archive take 200 MB, not 2 GB, and so do variations saved
from one another. The plugin finds the archive by the name in `meta.sbsar`, never
by where it is. A library made before this was converted when it was first
opened by the version that stores archives once (`tbx library info` says how much
that freed).

## Changing a material later

Open a material this plugin made and choose the **Substance** tab. It starts
from what the material was rendered with. Change the preset, parameters or
size, watch the preview, then either:

- **Update this material** — re-render in place. Tags, collections, source
  details and exports keep following it. The previous textures are replaced:
  the material's old folder is removed once the new one is in the library.
- **Save as new** — keep the original and add a second material that shares
  the archive.

## From the command line

Everything above can be done with [`tbx`](cli.md) too, and a little more. The
plugin's command-line module and the two panels call the same code
(`plugins/com.toybox.material-sbsar/lib/`): a material rendered from a terminal
is the same as one rendered in the window. `tbx` starts the engine when it
needs it and always stops it when the command ends; it removes the render
folders it made in the plugin's scratch folder (the app's own are left alone).

### Is the engine there?

```
tbx sbsar engine [--start] [--renderer auto|VK|CPU]
```

Shows where the engine is expected (the *Substance engine* setting), whether
the file exists, the *Render with* setting and whether something already
listens on the engine's port. With `--start` it starts the engine, waits for it
to answer and says how long that took and what it reported (the engine reports
no version). It exits `1` with a hint when the engine is not usable:

```
error: The Substance engine program was not found at C:\nope\engine.exe.
hint: install Adobe's Substance 3D Integration Tools, then set the program with:
      tbx plugin settings com.toybox.material-sbsar engine <path to substance_remote_engine>
```

`--renderer` (on this command, on `import sbsar`, `sbsar inputs` and `sbsar
render`) uses another renderer for this run only, when the run has to start the
engine. An engine that is already running keeps the renderer it started with.

### Looking inside an archive

```
tbx sbsar inputs <file.sbsar> [--preset NAME] [--graph N|LABEL]
```

Lists the archive's graphs, outputs and presets, and the parameters you can
set with their type, default, range, list entries (`1=Round`), switches and
colours. `--preset` adds the value that preset gives each parameter. The
texture size, the normal-map convention and the output switches are not
parameters here; the plugin sets them (`--size`).

### Importing

```
tbx import sbsar <PATH>... [--category NAME] [--size 512|1024|2048|4096]
                 [--name NAME] [--preset NAME | --all-presets] [--graph N|LABEL]
                 [--set NAME=VALUE]... [--values FILE.json] [--ignore-missing]
                 [--skip-existing] [--renderer auto|VK|CPU]
```

Paths are `.sbsar` files or folders (searched). Without options an archive is
rendered at its defaults at the size in the plugin's settings.

- **`--set name=value`** (repeatable) and **`--values file.json`** (one JSON
  object, name → value; applied first, so `--set` wins) lay parameter values
  over the defaults or the `--preset`. They are checked against the archive:

  | Kind | Written as |
  | --- | --- |
  | number | `0.4`, `12` (a whole number for an integer; a slider's range is enforced) |
  | switch | `true` / `false` (also `yes`/`no`, `on`/`off`, `1`/`0`) |
  | list | the entry's number or label: `--set glass_selection=Round` |
  | vector | numbers separated by commas: `--set tint=0.2,0.4,0.1` |
  | colour | the same, or `#rrggbb` / `#rrggbbaa` |
  | text | as it is |

  A wrong value is refused with what is allowed (`count: 99 is above the
  maximum (allowed 1 to 24)`, `glass_selection: "Hex" is not one of 0 =
  Square, 1 = Round`); an unknown name with the closest names (`did you mean
  joint_height?`). With several archives the failure belongs to that archive:
  it is skipped (exit `4`) and the others are imported. `--ignore-missing`
  leaves out a parameter an archive does not have, with a warning, instead.
- **`--graph`** renders another graph of an archive that holds several (by
  number or label); a material of a later graph is named `<archive> - <graph>`.
  The 61 archives tried so far hold one
  graph each, so this has only been exercised with a fake engine.
- **`--skip-existing`** leaves alone an archive whose material the library
  already has, rendered from the *same archive content* with the same graph,
  preset, parameters and size, and says how many it left alone (this is not an
  error: exit `0`). It makes re-running an interrupted batch cheap: with
  default parameters it does not even start the engine. The match is by the
  material's id (which follows the folder the archive was in and its name) and
  by a fingerprint of the archive recorded with the material, so a material made
  before the fingerprint existed is rendered once more.

A material's id comes from the folder the archive was in and the material's name,
so importing an archive again, with or without other parameters, **replaces** that
material (tags, collections and source details stay). Every import that does says
so, one line each, and `--json` lists the ids in `replaced`:

```
replaced: clay warm (was preset Dry Damaged Clay, 512 px)
Imported 1 item (0 new, 1 replaced) into 'Play'.
```

To keep earlier renderings give each its own name with **`--name`**: the material's
name, and with it its identity, instead of the archive's file name (with
`--all-presets` it is the base of `<name> - <preset>`). It names one material, so it
is refused, before anything renders, when more than one archive is selected:

```
tbx import sbsar clay.sbsar --name "clay warm" --preset "Dry Damaged Clay"
tbx import sbsar clay.sbsar --name "clay cool" --preset "Wet Dark Clay" --set surface_damage_amount=30
```

To vary a material that is already in the library use `tbx sbsar render --save-as`.

A failure says which archive and which step: `plain: loading in the engine: The
substance file [...] is not valid`, `tiles: rendering 2048 px: the engine
crashed while rendering`. Every archive that failed is listed as `skipped:` and
the command exits `4`.

### Rendering a material again

```
tbx sbsar render <SELECTION> [--set NAME=VALUE]... [--values FILE.json]
                 [--preset NAME | --reset] [--size 512|1024|2048|4096]
                 [--save-as NAME] [--ignore-missing] [--dry-run]
```

The same as the asset panel's *Update this material* (and with `--save-as`,
*Save as new*): it starts from what the material was rendered with, applies
your changes, renders, and replaces the material in place — same id, tags,
collections and details kept, old files removed. `--preset` starts from that
preset instead; `--reset` starts from the archive's defaults. `--save-as NAME`
keeps the original and adds a new material (one item only; it copies the
original's source details). Items that are not Substance materials, or whose
archive is not stored with them, are skipped with the reason (exit `4`).
`--dry-run` opens each archive in the engine, checks the values and prints what
each material would be rendered with, rendering nothing.

```
tbx sbsar render --type material --tag floor --preset "Dark Dirty" --size 1024
tbx sbsar render <ID> --set planks_wobble_scale=20 --set "wood_colour_a=#331100" --save-as "dark planks"
```

### Showing how materials were made

```
tbx sbsar show <SELECTION>        (add --json for data)
```

For each material: the archive, graph, preset, the parameters changed from the
preset or defaults, the size, and whether the archive is stored with it.

## How it works

The engine listens on a local port and takes JSON messages. The plugin starts
it when first needed (the host stops it when Toybox closes), loads the archive,
sets parameters, and asks for a render into the plugin's scratch folder. It
then copies the maps a material uses, and the archive, into the library.

Notes from working with the engine, all handled in
`plugins/com.toybox.material-sbsar/lib/engine.ts`:

- A render must request every output of the graph; a subset stops the engine.
- The engine announces a finished render to an HTTP callback. The plugin
  watches the output folder instead and reads a file only once it is complete.
- After any parameter change the engine reports each input's *current* value
  as its default, so real defaults are taken from the first load only.
- On Windows the engine's OpenGL renderer writes red and blue swapped.
  Direct3D, Vulkan and the processor renderer do not, so OpenGL is never used
  there.
- Normal maps are requested in the OpenGL convention, which is what the
  library stores.

Everything above lives in the plugin. The host contributes only general
services any plugin can use — see
[Plugins that drive an external program](asset-types.md#plugins-that-drive-an-external-program).
