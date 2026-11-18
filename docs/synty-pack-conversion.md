# Converting Synty asset packs into the Toybox library

How a raw Synty asset pack (loose FBX + textures + a MaterialList) becomes a
Toybox library pack (`library/<pack>/<category>/<name>.gltf`). Written from the
one conversion done in practice: **Polygon SciFi Space** (`D:\code\games\synty`)
→ 665 assets in pack `polygon_scifi_space`.

> Status: this is a **standalone toolkit pipeline**, not an in-app importer.
> Why it can't (yet) be a Toybox import plugin is covered at the end.

---

## What a raw Synty FBX pack looks like

A Synty "source" pack (the FBX export, *not* a `.unitypackage`) contains:

```
<pack>/
  FBX/                         # 600+ static-mesh FBX (props, buildings, vehicles…)
    FX_Meshes/                 # a handful of FX meshes
  Characters/                  # SK_Chr_*.fbx — rigged (skeletal) character FBX
  Textures/                    # shared atlas PNGs (+ Alts/, FX_Textures/ subdirs)
  MaterialList_<Pack>.txt      # the prefab → mesh → material/texture map
  Demonstration_*.fbx/.ma      # huge demo scenes — IGNORE these
```

Key facts that shaped the pipeline (measured on SciFi Space):

- **Loose FBX, no Unity GUIDs.** The older `.unitypackage` flow
  (`build-library.ts`) resolves a prefab→material→texture GUID chain. Raw FBX
  packs don't have that; the **MaterialList text file** is the binding source
  instead.
- **One dominant atlas.** ~94% of all material slots reference a single texture
  (`PolygonSciFiSpace_Texture_01_A`); only ~3 distinct textures total. Most
  assets are trivially one-atlas.
- **Characters are rigged but (mostly) unanimated.** FBX2glTF preserves the
  skeleton + skin weights. Most ship no clip; a few (`SK_Chr_BR_*`) carry a real
  baked "Take 001".

### The MaterialList grammar

`MaterialList_<Pack>.txt` is line-based and indentation-sensitive:

```
Folder Name: Buildings
Prefab Name: SM_Bld_Bridge_Chair_01
    Mesh Name: SM_Bld_Bridge_Chair_01
        Slot: PolygonScifiSpace_Material_01_A (PolygonSciFiSpace_Texture_01_A)
```

- `Folder Name:` → the **category** (Buildings, Characters, Environment, FX,
  Props, Signs, Vehicles, Weapons, Attachments).
- `Prefab Name:` → one asset.
- `Mesh Name:` → a sub-mesh of that prefab (a character lists many).
- `Slot: <Material> (<inner>)` → `<inner>` is the **texture name**, OR a marker:
  `Uses custom shader`, `No Albedo Texture` → treat as *no texture*.

(`parseMaterialList` in `src/pack/material-list.ts` already parses this — it was
written for PolygonPrototype and works unchanged on SciFi Space.)

---

## The pipeline

Two scripts in the rangers toolkit (`D:\code\games\rangers\mecha-rangers\tools`,
Bun + TypeScript):

### 1. `build-fbx-pack.ts` — convert FBX → glTF into the library

```sh
bun run src/pack/build-fbx-pack.ts <packDir> \
  --theme polygon_scifi_space \
  --out D:/code/games/assets \
  [--material-list <file.txt>] [--scope <regex>] [--limit N] [--dry-run]
```

Per FBX:

1. **Look up** its MaterialList entry → category + per-mesh texture bindings.
2. **Convert** FBX → geometry-only `.gltf` + `.bin` via the vendored
   `bin/FBX2glTF.exe` (reused `convertFbx` in `src/pack/fbx2gltf.ts`), flags
   `--pbr-metallic-roughness --separate-textures`.
3. **Strip the empty take.** FBX2glTF emits an empty `Take 001` (0 channels) for
   static + rigged-but-unanimated meshes; drop any animation with no channels.
   Real clips (channels > 0) are kept.
4. **Patch textures** (reused `patchGltf` in `src/pack/gltf-patch.ts`) by
   mesh-name binding; copy referenced `Textures/<name>.png` into
   `library/<theme>/textures/` (deduped across the whole pack).
5. **Write** `library/<theme>/<category>/<name>.gltf` + `.bin`.

Always `--dry-run` first (lists what would be processed, no writes), then a
small `--scope` subset, then the full run.

### 2. `register-pack.ts` — register the converted pack

```sh
bun run src/pack/register-pack.ts \
  --assets D:/code/games/assets \
  --theme polygon_scifi_space \
  --name "Polygon SciFi Space" \
  [--color 7b61ffff] [--dry-run]
```

- Scans `library/<theme>/<category>/*.gltf`, appends one catalog entry per glTF
  (idempotent, keyed on path+name; synthesizes stable `uid://` ids).
- Writes the pack's **`pack.json`** (`{ slug, name, color }`) into the pack dir —
  Toybox reads this for the pack's display name/color. This is the source of
  truth for pack identity, independent of any Godot file.

> NB: the in-app catalog seed was `_library_config/catalog.json` (the
> `{ id, pack, category, file }` shape), not the old Godot `asset_library.json`.
> Since migration step 4 a library's entries are rows of
> `_library_config/library.db` (`items.entry` holds the same entry JSON), and a
> library that has been opened by a current Toybox has no `catalog.json` any more.
> A register step that appends to `catalog.json` therefore no longer works against
> such a library, and the database must not be edited by hand. This document does
> not re-establish how to register a converted pack now: bring the converted glTF
> files in through Toybox (the glTF importer, which commits entries the same way
> any importer does), and see [SQLite library storage](sqlite-storage.md). The
> `register-pack.ts` script is not in this repository, so it was not checked.

---

## Gotchas (all hit during the SciFi Space conversion)

- **Space Soldier source/prefab names also reorder suffixes.** For example,
  `SK_Chr_SpaceSoldier_Male_Helmet` corresponds to the prefab
  `SM_Chr_SpaceSoldier_Helmet_Male_01`. Prefix swapping alone misses it.
  Its actual glTF mesh names still match MaterialList mesh names after `SK_`
  becomes `SM_`, so missing bindings can be restored from those explicit,
  unambiguous mesh entries. A
  texture-existence check alone cannot find this omission: untextured files
  have no image references to check. Audit used material slots too.

- **`SK_Chr_*` FBX vs `SM_Chr_*` MaterialList names.** Character files are named
  `SK_Chr_Alien_01.fbx` (skeletal) but the MaterialList lists them as
  `SM_Chr_Alien_01` (static-mesh naming). Lookup falls back by swapping the
  `SK_` → `SM_` prefix. Without it, all ~58 characters convert untextured.

- **Modular characters export as ONE merged glTF mesh.** A character FBX bundles
  dozens of attachment sub-meshes, but FBX2glTF emits a single mesh named after
  the FBX (`SK_Chr_Crew_Male_01`) — matching none of the MaterialList's per-part
  mesh names. So `patchGltf`'s name-based binding applies to *nothing* and the
  character is untextured. Fix: a **dominant-texture fallback** — bind every
  actual glTF mesh to the entry's most-referenced texture when the per-mesh names
  don't line up.

- **Empty "Take 001" must be stripped.** FBX2glTF always emits a default take.
  Left in, every static prop looks "animated" (clip count 1) but the clip is
  empty. Filter `animations` to channels > 0; delete the array if it empties.

- **Real animations DO survive.** The 6 `SK_Chr_BR_*` characters ship a genuine
  baked clip — those are correctly preserved (and show up animated in Toybox).
  Don't strip indiscriminately.

- **Skip the demo scenes.** `Demonstration_Exterior.fbx` / `_Interior.fbx` at the
  pack root are giant combined scenes, not assets. Exclude by name.

- **Disk space.** A full pack conversion writes hundreds of MB; FBX2glTF also
  leaves `tools/scratch/fbx2gltf/*` temp dirs behind on *failed* conversions
  (cleanup only runs on success). The SciFi Space run hit `ENOSPC` partway
  (drive was at 100%); after freeing space the re-run completed clean. Check free
  space first; the scratch temp dirs are safe to delete.

- **Back up the catalog seed** before `register-pack` (`.bak`). It's append-only,
  but verify the existing packs' counts are untouched afterward. (A library
  that has `library.db` instead of `catalog.json` has no such file to copy; to back
  it up, copy `library.db` with Toybox closed, and do not edit it by hand.)

---

## Why this isn't an in-app import plugin (yet)

Toybox has a plugin system whose importers ship an import panel (see
`plugins/com.toybox.model-core`, which hands each file to a format plugin such as
`plugins/com.toybox.model-gltf`). The glTF format works because glTF/GLB needs
**no conversion** — it just copies/unpacks files via the jailed `ctx.fs`, and the
panel then calls `ctx.commit(entries)`.

FBX is different: it requires running **FBX2glTF.exe**, a native binary. The
plugin sandbox is deliberately jailed:

- `ctx.fs` reads/writes *bytes* within scoped roots — it cannot execute anything.
- `ctx.host` (`HostApi`) has a fixed primitive set (`assembleGlb`, `performCopy`,
  `transcodeImage`, …) — none spawn a subprocess.

So even a UI where the user points at their `FBX2glTF.exe` (easy — a
`directory`/file field) doesn't unblock it: the plugin has no capability to
*run* that exe. Executing a user-pointed binary is exactly what the sandbox
prevents.

**To make it a plugin would require a host-contract extension**: a Rust-backed
primitive like `ctx.host.convertFbx(exePath, fbxBytes) -> { gltf, bin }` that
does the spawn on the trusted side, gated by a new permission. With that, the
Synty importer becomes a normal panel-only plugin (pick pack folder → per FBX
call `host.convertFbx` → patch via existing primitives → `ctx.commit`). That's a
deliberate design step, not a drop-in — tracked as future work.

Until then: **run the toolkit pipeline above**, then point the Models import
(with the glTF models plugin turned on) at the converted `library/<theme>/`
folder, or let `register-pack.ts` register it directly.
