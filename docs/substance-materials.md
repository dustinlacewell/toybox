# Substances (.sbsar)

The bundled **Substances** plugin (`com.toybox.material-sbsar`, which depends on
**Materials**, `com.toybox.material-core`) brings Substance archives into the
library. An `.sbsar` is a procedure, not a set of images: the plugin has Adobe's
Substance engine render it into colour, normal, roughness, metal, shadowing and
height maps, with the preset and parameter values you choose.

Two kinds of item come out of it:

- a **substance** — the archive itself, listed under **Substances**. One archive
  is one substance per library, however often and from wherever it is imported:
  its identity is the archive's content.
- **materials** rendered from it — ordinary library materials, listed under
  **Materials** next to materials from texture folders. They preview, tag,
  collect and export exactly the same. Each names the substance it was made from.

One substance can have any number of materials: presets, variations, sizes.

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

Toybox does not ship or download the engine. **Settings → Plugins → Substances
→ Preferences** holds its location:

- **Substance engine** — the program file. It defaults to the Windows location
  above; choose the file yourself anywhere else.
- **Render with** — *Automatic* uses the graphics card. Vulkan and *Processor
  only* are there for machines where that fails.
- **Default texture size** — the size the import screen and a new material
  start at, and the size `tbx import sbsar` renders at without `--size`: 1024
  pixels unless you change it.

Browsing substances and their materials needs no engine; previewing and
rendering do.

## Importing

**Import → Substances (.sbsar)**, then choose a folder. Every `.sbsar` in it is
listed and ticked. Selecting one opens it in the engine and shows:

- a live preview, rendered at 512 pixels and redrawn as you change things;
- **Preset** — the presets the archive's author shipped;
- the parameter editor (described under [The substance viewer](#the-substance-viewer));
- **Import all presets as separate materials** — one material per preset,
  named `<archive> - <preset>`, each as its author made it.

**Texture size** on the left applies to the import. Ticked archives you never
opened are imported at their defaults.

Each archive is added as a substance, named after its file, with its default
material (or one per preset). An archive the library already has — the same
content, under any file name or folder — is not stored again: the new materials
are added to the substance that is there.

## The substance viewer

Open a substance to see and edit what is made from it. The stage shows a live
preview of the material being edited; drag to look around. The **Substance**
panel beside it holds, from top to bottom:

1. **Materials** — every material made from this substance, with its picture,
   name, preset and size, and **New material** at the top. Choose one to edit it;
   **Open** shows that material in the viewer instead. Choosing the material
   being edited again throws away your unsaved changes to it.
2. **Rendering** — the graph (only for an archive that holds several), the
   preset and the texture size. A material keeps the graph it was rendered
   from: choosing it shows that graph, and choosing another graph while a
   material is chosen turns to **New material** with that graph's defaults
   (never converting the material). A material whose record cannot be read
   is shown from the defaults of the first graph, with a note that its
   settings are unknown.
3. **Parameters** — the archive's parameters, each with the control its author
   gave it: a slider with a number box, a switch, a list, a colour (with its hex
   value), the components of a vector, text, or the random seed with
   **Shuffle**. The archive's groups fold away; search filters by name and
   group. A parameter that differs from the preset is marked *changed* and has a
   reset button; **Reset all** returns every parameter to the preset. Parameters
   the archive hides in its current state are not shown, as in Substance's own
   tools. Hover a parameter for its author's description.
4. At the bottom, what to do with it:
   - with a material chosen: **Update *name*** renders it again in place — same
     item, so tags, collections, source details and exports keep following it;
     its previous textures are replaced. **Save as new material…** keeps it and
     adds another material of the substance under a name you give.
   - with **New material** chosen: a name (the substance's name, with the
     preset, made unique) and **Create material**.

   A new material's name may not be one the substance's materials already
   have (ignoring case): to change that material, choose it and update it.
   After **Create material** or **Save as new material** the new material is
   the one being edited straight away; its **Update** button appears once the
   refreshed list shows it.

The final render happens at the texture size chosen, which can take a while for
4096 pixels; the panel says what it is doing, and the last preview stays on the
stage meanwhile.

On a remote library the archive is downloaded when the substance is opened (an
archive can be hundreds of megabytes); the library grid shows a substance from
its small preview picture and never needs the archive.

## A material's Substance tab

A material made from a substance has a **Substance** tab in its viewer: the
substance (picture and name), the preset, the texture size and the parameters
changed from the preset. **Open substance** opens the substance viewer with this
material chosen, ready to edit. Nothing is rendered in the tab itself.

If the substance has been deleted the tab says so: the material stays as it is,
but cannot be rendered again. Deleting a substance never deletes its materials,
and deleting materials never touches the substance.

## Where the files live

```
library/substance/<key>/
  ceramic_tiles_blue.sbsar
  preview.png
library/materials/<key>/
  albedo.png  normal.png  roughness.png  metallic.png  ao.png  height.png
```

The substance keeps the archive and `preview.png`, the colour map of the first
material imported with it (at most 512 pixels: a larger render is scaled down;
where that fails the substance has no preview). Its library thumbnail is the
host's material sphere, identical to a material's own thumbnail: the sphere of
its default material (of the materials made from it, the one of the first
graph with no preset and no changed inputs, else the one with the fewest
changed inputs, ties by name), rendered from that material's maps (on a remote
library only the maps are downloaded, never the archive). With no such
material, or when its maps cannot be had, the sphere carries `preview.png` as
its colour; with no preview either, it is a neutral tile with the substance
glyph. Adding, replacing or deleting a material made from a substance redraws
the substance's thumbnail. Nothing of a substance is staged
until a material of its archive has rendered, so an archive that fails leaves
nothing behind. Its
entry records the archive's name, SHA-256 and size, and, when it was imported by
this version, a summary of each graph (presets, parameter count, outputs):

```json
{ "kind": "plugin", "type": "substance", "id": "uid://substance_<first 32 hex digits of the sha256>",
  "name": "ceramic_tiles_blue", "format": "sbsar",
  "files": ["library/substance/<key>/ceramic_tiles_blue.sbsar", "library/substance/<key>/preview.png"],
  "meta": { "sbsar": "ceramic_tiles_blue.sbsar", "sha256": "4f0c…", "bytes": 258588243,
            "graphs": [{ "index": 0, "label": "Ceramic Tiles", "presets": ["Commander Green"],
                         "parameters": 14, "outputs": ["baseColor", "normal", "roughness"] }] } }
```

A material keeps only its maps. It names its substance and records how it was
rendered:

```json
"origin": { "plugin": "com.toybox.material-sbsar",
            "derivedFrom": "uid://substance_4f0c…",
            "meta": { "sbsar": "ceramic_tiles_blue.sbsar", "graph": 0, "preset": "Commander Green",
                      "inputs": { "joint_height": 0.3 }, "size": 11 } }
```

Ten presets of a 200 MB archive take 200 MB, not 2 GB. The archive is not part
of an export — a game project gets the textures. Copying a material to another
library takes its substance along when that library does not have it yet.

### Libraries made before substances

Earlier versions kept the archive with its materials, in
`library/materials/_sources/<sha256>/`. A library is converted the first time
this version opens it: each archive becomes a substance — **moved** into
`library/substance/<key>/`, never copied, so the conversion needs no free space
— named after its material when only one material used it, otherwise after the
archive's file. Its materials then name it; their ids, names, tags and textures
do not change. A substance made this way has no graph summary until the archive
is imported again (that adds nothing new: it is the same substance). `tbx
library info` reports the conversion.

## From the command line

Everything above can be done with [`tbx`](cli.md) too. The plugin's
command-line module, the import panel and the viewer call the same code
(`plugins/com.toybox.material-sbsar/lib/`): a material rendered from a terminal
is the same as one rendered in the window. `tbx` starts the engine when it
needs it and always stops it when the command ends; it removes the render
folders it made in the plugin's scratch folder (the app's own are left alone).
The app cannot remove one folder at a time: it empties the scratch folder when
no render of its own is in use (once per session, then after every few
previews), which can also take the folder of a `tbx` render running at that
moment.

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
rendered at its defaults at the size in the plugin's settings. Each archive the
library does not have yet becomes a substance (with the import's category, named
after the archive's file, or `--name`); its materials come after it.

Everything committed is printed and counted, as for any importer: the ids on
standard output, `Imported N items (x new, y replaced)` and `imported` under
`--json` include a substance added on the way (before its materials). Such a
substance is also said on standard error, on a line of its own:

```
Added 1 substance (archives new to this library): ceramic_tiles_blue
```

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
  The 61 archives tried so far hold one graph each, so this has only been
  exercised with a fake engine.
- **`--skip-existing`** leaves alone an archive whose material the library
  already has, made from the *same substance* (so the same archive content) with
  the same graph, preset, parameters and size, and says how many it left alone
  (this is not an error: exit `0`). It makes re-running an interrupted batch
  cheap: with default parameters it does not even start the engine — it hashes
  the archive and looks up the substance and the material. The material is found
  by its id (which follows the folder the archive was in and its name). A
  material whose substance has been deleted is rendered again, and the substance
  comes back with it.

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
the command exits `4`; an archive nothing could be rendered from is not added
as a substance either.

### Rendering again

```
tbx sbsar render <SELECTION> [--set NAME=VALUE]... [--values FILE.json]
                 [--preset NAME | --reset] [--size 512|1024|2048|4096]
                 [--save-as NAME [--replace]] [--ignore-missing] [--dry-run]
```

For a material, the same as the viewer's *Update* (and with `--save-as`, *Save
as new material*): it opens the archive of the material's substance, starts from
what the material was rendered with, applies your changes, renders, and replaces
the material in place — same id, tags, collections and details kept, old files
removed. `--preset` starts from that preset instead; `--reset` starts from the
archive's defaults. `--save-as NAME` keeps the original and adds a new material
of the same substance (one item only; it copies the original's category and
source details).

A selected **substance** is rendered into a new material of its own and needs
`--save-as NAME`; it starts from the archive's defaults, or `--preset`, at
`--size` or the plugin's default size.

`--save-as` refuses a name one of the substance's materials already has
(compared ignoring case, after the name is cleaned), as the viewer does: the
item is skipped with the reason (exit `4`). Add `--replace` to replace that
material instead: it keeps its id (so its tags and collections), takes the new
name and rendering, and is reported under `replaced` (and `replaced: <name>
(<id>)` on standard error) rather than `created`. `tbx import sbsar` is
different on purpose: importing again replaces the material of the same name
and says so.

Items that are not Substance materials, whose substance is no longer in the
library, or whose library has not been converted yet, are skipped with the
reason (exit `4`). On a remote library the substance's archive is downloaded
first. `--dry-run` opens each archive in the engine, checks the values and
prints what each item would be rendered with, rendering nothing.

```
tbx sbsar render --type material --tag floor --preset "Dark Dirty" --size 1024
tbx sbsar render <ID> --set planks_wobble_scale=20 --set "wood_colour_a=#331100" --save-as "dark planks"
tbx sbsar render <SUBSTANCE ID> --preset "Wet Dark Clay" --save-as "clay wet"
```

### Showing how things were made

```
tbx sbsar show <SELECTION>        (add --json for data)
```

For each material: its substance (name and id, or that it is gone), the
archive, graph, preset, the parameters changed from the preset or defaults, the
size, and whether the archive is stored. For each substance: the archive and
its size, its graphs with their presets, parameter counts and outputs when
known, and every material made from it with its preset and size.

## How it works

The engine listens on a local port and takes JSON messages. The plugin starts
it when first needed (the host stops it when Toybox closes), loads the archive,
sets parameters, and asks for a render into the plugin's scratch folder. It
then copies the maps a material uses into the library, and for a new substance
the archive and a preview picture.

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

The plugin declares the Substances type itself (see
[asset types](asset-types.md)); the link from a material to its substance is the
host's general `origin.derivedFrom`. The host contributes only general services
any plugin can use — see
[Plugins that drive an external program](asset-types.md#plugins-that-drive-an-external-program).
