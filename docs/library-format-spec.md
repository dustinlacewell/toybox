# Toybox Asset Library Format — Conversion Spec for LLM Agents

> **Legacy inbound format:** the pack-folder layout in this document predates
> managed storage in Toybox 0.4.1. It can describe conversion input, but it is not
> the final library layout. For existing libraries, follow the
> [AI migration guide](ai-library-migration.md); for the managed storage contract,
> see [library-storage.md](library-storage.md). Do not recreate pack folders as
> the final storage organization or skip the native migration.
>
> **Where the `catalog.json` described here ends up.** Toybox no longer keeps a
> library's manifest as `_library_config/catalog.json`. Since migration step 4
> (`sqlite-store`) the items are rows of `_library_config/library.db` (`items.entry`
> holds the entry JSON; see [SQLite library storage](sqlite-storage.md)). A
> `catalog.json` that you author in this document is **inbound input only**: when
> Toybox opens a library folder that has a `catalog.json` and no `library.db`, it
> converts the file into `library.db` (and runs the earlier migration steps) and
> moves the original into `_library_config/migration-backups/0004-<stamp>/`. Once
> `library.db` exists, never hand-edit it and never recreate `catalog.json` next to
> it; add or change entries through Toybox (the importer commit and the metadata
> commands). Reading `library.db` with a SQLite tool in read-only mode is fine.
> Importer plugins still produce entries of the shape specified below.

You are converting a **foreign 3D-asset collection** (some other library, a bundle of
asset packs, a pile of loose models — anything) into the **Toybox library format**. This
document describes the legacy inbound conversion contract. Use the current app's
import/commit path or its native migration helper to produce final managed storage.

Read the whole document once before starting. The format is small; the discipline is in
getting paths and the source manifest exactly right.

---

## 1. What you're producing — the mental model

A Toybox library is a **directory** with two parts:

```
<library-root>/
├── _library_config/
│   └── catalog.json          ← the inbound manifest YOU author (the only hand-written file;
│                                Toybox turns it into library.db when it first opens the library)
└── library/
    ├── <pack>/
    │   ├── pack.json          ← per-pack identity (name, color)
    │   ├── <category>/
    │   │   ├── Foo.gltf       ← the model
    │   │   ├── Foo.bin        ← its binary buffer
    │   │   └── ...
    │   └── materials/         ← material sets (see material-set-format.md)
    └── <pack>/
        └── textures/          ← textures may live anywhere the glTF points
            └── Atlas.png
```

Toybox **derives** almost everything by reading the files. You author only:

1. The on-disk file tree under `library/<pack>/<category>/`.
2. One `pack.json` per pack.
3. One `catalog.json` listing every model by four fields.

Material sets use a different entry and folder. They are described in
`material-set-format.md`. Independent animation extraction, rig matching and
animation-only source entries are described in `animations.md`. The rest of
this document covers models.

An entry may include a `variants` array of complete entries for additional file
sets. The primary entry is the best available detail level or largest material
size. Variants must have unique IDs and share the primary's kind, pack and
category. They cannot contain nested variants. They appear under one library
tile, and exporters receive the primary plus its variant list. Different colour
materials remain separate assets; material assignments belong to the game.

```json
{
  "id": "uid://tree",
  "pack": "forest",
  "category": "props",
  "file": "Tree_LOD0.gltf",
  "variants": [
    { "id": "uid://tree_low", "pack": "forest", "category": "props", "file": "Tree_LOD2.gltf" }
  ]
}
```

Importer commits upsert this same entry format (into `library.db`). When an import
groups former standalone entries into variants, those former top-level rows are
absorbed. Unrelated entries remain in the library.

Everything else — each asset's texture set, its `.bin`, its animation clips, thumbnails,
user tags — Toybox computes or owns. **Do not try to author those.** Your job ends at the
three things above.

### Source files kept with an item

An importer may keep the file an item was made from (a material's `.sbsar`, a model's
original `.fbx` and its companions) in the entry's `origin.files`: bare file names staged
next to the entry's own files. When the entry is committed Toybox stores each distinct
file once per library, by content, in `library/<materials|models>/_sources/<sha256>/<name>`
and the entry then lists those paths instead (so several items made from one archive share
one copy). Do not author the shared paths; stage bare names and let Toybox commit.

### The cardinal rule

> **`catalog.json` is a manifest of files that already exist on disk.** Every model entry
> must point at a real `.gltf` under `library/<pack>/<category>/`, and that glTF's referenced
> buffer and textures must also exist where the glTF says they are. Toybox reads each glTF
> during ingestion; a missing file is a hard error, not a warning.

So the workflow is always: **lay down the files first, then write the manifest that
describes them.**

---

## 2. The model format: glTF (`.gltf` + `.bin`, separate)

Every model is a **glTF 2.0** file in the **`.gltf` + `.bin` separate-file** form — *not*
embedded `.glb`, *not* base64-inlined buffers.

- The model is `<Name>.gltf` (JSON).
- Its binary buffer is a sibling `<Name>.bin`, referenced from the glTF's
  `buffers[].uri`.
- Textures are external image files referenced from `images[].uri`.

If your source assets are in another format (FBX, OBJ, `.glb`, USD, embedded glTF, …) you
**must convert them to separate-file glTF 2.0 first.** Recommended converters:

| Source           | Tool                                                              |
| ---------------- | ---------------------------------------------------------------- |
| FBX              | `FBX2glTF` (Facebook/Khronos), or Blender export                 |
| OBJ / others     | Blender (import → glTF Separate export), or `assimp`              |
| `.glb` (binary)  | `gltf-transform separate <in.glb> <out.gltf>` (glTF-Transform)   |
| embedded `.gltf` | `gltf-transform separate` to externalize the buffer + images     |

When exporting/converting, require the **"glTF Separate"** variant (`.gltf` + `.bin` +
loose textures), not "glTF Binary" (`.glb`) and not "glTF Embedded".

### Texture URIs — the one thing that trips converters

Toybox resolves each texture by reading `images[].uri` from the glTF and resolving it
**relative to the directory containing the glTF**, normalizing `.` and `..` segments. So
these are all valid and Toybox will find them:

- `images[].uri = "Atlas.png"` → sibling of the glTF.
- `images[].uri = "../textures/Atlas.png"` → in a `textures/` dir one level up (i.e.
  `library/<pack>/textures/Atlas.png`). **This is the conventional layout** — a shared
  `textures/` folder at the pack root, used by all categories.
- `images[].uri = "./maps/Foo.png"` → a `maps/` subfolder beside the glTF.

Rules you must honor:

- **URIs must be relative.** No absolute paths, no `file://`, no `http(s)://`, no
  drive letters.
- **The file the URI points at must actually exist on disk** at the resolved location.
- **Texture format:** PNG and JPG are first-class. A `.tga` reference is tolerated (Toybox
  can transcode it for some export paths) but prefer PNG/JPG when you control the
  conversion. Avoid exotic formats (EXR, DDS, KTX2) — convert them to PNG.
- **Shared atlases are fine and encouraged.** Many assets pointing at the same
  `../textures/Atlas.png` is the normal case; Toybox de-duplicates texture references
  automatically. Do **not** copy a shared atlas once per asset.

### Animation

If a glTF carries animation, leave it in. Toybox reads `animations[]` and records clip
names per asset automatically — **only clips that actually have channels count.** You don't
declare animation anywhere; it's derived. Empty/placeholder takes (e.g. an FBX2glTF
`"Take 001"` with no channels) are ignored by Toybox, so strip them if your converter emits
them, but it's not fatal if you don't.

---

## 3. Directory layout under `library/`

```
library/<pack>/<category>/<File>.gltf
```

This three-level shape is **mandatory and load-bearing** — Toybox reconstructs each asset's
path as exactly `library/<pack>/<category>/<file>` from the manifest. There is no field for
an arbitrary path; the path *is* `library/<pack>/<category>/<file>`.

- **`<pack>`** — a slug: lowercase, `snake_case`, no spaces (e.g. `polygon_city`,
  `medieval_village`, `my_kitbash`). This is the directory name **and** the `pack` field in
  every asset entry **and** the `slug` in `pack.json`. All three must match exactly.
- **`<category>`** — a slug grouping assets within a pack (e.g. `buildings`, `props`,
  `characters`, `vehicles`, `vegetation`, `weapons`). Lowercase `snake_case`. Choose a
  small, consistent set per pack — these become the filter facets in the UI. The name
  `materials` is reserved: `library/<pack>/materials/` holds material sets, so a model
  category must not use it.
- **`<File>.gltf`** — the model filename. The display name is the filename without
  `.gltf`. Keep names unique *within their category folder* (filenames may legitimately
  collide across different packs — that's why the manifest uses an explicit `id`, see §5).

The sibling `<File>.bin` lives next to the `.gltf`. Textures live wherever the glTF's URIs
point (conventionally `library/<pack>/textures/`).

**Categories are your call.** If the foreign library has its own taxonomy, map it to
sensible category slugs. When in doubt, fewer broad categories beat many tiny ones. Keep
the same category vocabulary consistent across a pack.

---

## 4. `pack.json` — one per pack

Place at `library/<pack>/pack.json`. Three fields, all required:

```json
{
  "slug": "polygon_city",
  "name": "Polygon City",
  "color": "4f9fffff"
}
```

- **`slug`** — must equal the `<pack>` directory name and the `pack` field used by this
  pack's assets in `catalog.json`. Exact match.
- **`name`** — human-facing display name (free text, spaces allowed).
- **`color`** — a display accent color as **Godot-style `rrggbbaa` hex** (8 hex digits,
  no `#`). The trailing `aa` is alpha; use `ff` for opaque. E.g. `4f9fffff` is opaque
  blue. Pick any distinct color per pack.

A pack directory without a `pack.json` is silently ignored as a pack, so don't forget it.

---

## 5. `catalog.json` — the manifest you author

Place at `<library-root>/_library_config/catalog.json`. This is the inbound source document
Toybox seeds from: it is read once, on the first open of a library folder that has no
`_library_config/library.db`, and then moved to the migration backup (see the note at the
top). Do not write it into a library that already has `library.db`. Shape:

```json
{
  "schemaVersion": 1,
  "assets": [
    {
      "id": "uid://p6p3bf5qgcvb",
      "pack": "polygon_city",
      "category": "buildings",
      "file": "SM_Bld_Apartment_01.gltf"
    },
    {
      "id": "uid://7h2k9xmqab3v",
      "pack": "polygon_city",
      "category": "props",
      "file": "SM_Prop_Bench_02.gltf"
    }
  ]
}
```

### Top level

- **`schemaVersion`** — integer literal **`1`**. (This is the *source-document* schema
  version; it is not the same number as Toybox's internal catalog version. Always write
  `1`. After conversion it is kept as `manifestSchemaVersion` in `library.db`.)
- **`assets`** — array of asset entries. Order is irrelevant.

### Each model entry — exactly four string fields

An entry may also have an optional string field `kind`. If `kind` is absent or `"model"`,
the entry is a model and this section applies. If `kind` is `"material"`, the entry is a
material set: read `material-set-format.md`. Any other value is an error.

| Field      | Meaning                                                                 |
| ---------- | ----------------------------------------------------------------------- |
| `id`       | **Stable, globally-unique primary key.** See below. Never reused.       |
| `pack`     | Pack slug. Must equal a `pack.json` `slug` and a `library/<pack>` dir.  |
| `category` | Category slug. Must equal the `<category>` dir the file sits in.         |
| `file`     | The glTF filename **with `.gltf` extension** (no path, just the name).  |

Toybox reconstructs the asset's location as
`library/<pack>/<category>/<file>` and its buffer as the sibling
`library/<pack>/<category>/<name>.bin` (where `<name>` is `<file>` minus `.gltf`). So:

- `file` is a **bare filename**, never a path. `"SM_Bld_Apartment_01.gltf"`, not
  `"buildings/SM_Bld_Apartment_01.gltf"`.
- `file` **must end in `.gltf`.**
- The `pack`/`category` in the entry must match the directory the file actually sits in.
  If they disagree with the on-disk location, the glTF read fails.

### The `id` field — get this right

`id` is the **primary key** Toybox identifies assets by. glTF filenames collide across
packs, so the catalog keys on `id`, not on name or path.

- It must be a **string**, **globally unique** across the entire catalog, and **stable** —
  the *same* asset must keep the *same* `id` across re-imports, or Toybox will treat it as
  a new asset and the user loses any tags/favorites they attached.
- The convention is a Godot-style `uid://` token (`uid://` + a short base-ish suffix,
  e.g. `uid://p6p3bf5qgcvb`). You are **not required** to use that exact scheme — any
  stable unique string works — but matching the convention keeps the data uniform.
- **How to mint stable ids when the source has none:** derive deterministically from
  something invariant about the asset, e.g. hash `"<pack>/<category>/<file>"` (or the
  source asset's own GUID/UID if it has one) and format as
  `uid://<first-12-chars-of-hash>`. Deterministic derivation means re-running your
  converter produces identical ids — which is exactly the stability requirement.
- If the source library already carries stable per-asset identifiers (Unity GUIDs, Godot
  uids, content hashes), **prefer those** — wrap them as `uid://<their-id>` so they stay
  stable forever.

Do **not** mint random ids at convert time unless you persist them; a fresh random id on
every run breaks stability.

### What does NOT go in the manifest

A model entry has **only** those four fields (plus the optional `kind`). Do not add `textures`, `bin`, `path`, `name`,
`animation`, `tags`, `thumb`, or anything else — Toybox derives or owns all of it:

- **Name** = `file` minus `.gltf`.
- **Texture list** = read from the glTF's `images[].uri`, de-duplicated.
- **`.bin`** = the sibling buffer.
- **Animation clip names/count** = read from the glTF's `animations[]`.
- **Thumbnails** = rendered by Toybox after import.
- **Favorites / user tags** = owned and persisted by Toybox; re-imports preserve them
  (keyed on your stable `id` — another reason ids must be stable).

Adding extra fields isn't fatal (they're ignored), but it signals a misunderstanding —
keep entries to the four fields.

---

## 6. Validation checklist — run through this before declaring done

Lay out the files, write the manifests, then verify **all** of these. Toybox fails the
import on the first violation, so check them yourself first.

**Structure**

- [ ] `<library-root>/_library_config/catalog.json` exists and is valid JSON, and
      `_library_config/library.db` does not exist (a new, unopened library).
- [ ] Every pack has `library/<pack>/pack.json` with `slug` == the directory name.
- [ ] Every asset's `.gltf` exists at `library/<pack>/<category>/<file>` matching its
      manifest `pack`/`category`/`file` exactly.

**Per glTF**

- [ ] It's separate-file glTF 2.0 (a JSON `.gltf`, not `.glb`, not embedded).
- [ ] Its `buffers[].uri` `.bin` exists at the resolved location (normally the sibling
      `<name>.bin`).
- [ ] Every `images[].uri` is **relative** and the resolved image file **exists** on disk.
- [ ] No absolute paths / `file://` / URLs / drive letters anywhere in the glTF URIs.

**Manifest**

- [ ] `schemaVersion` is `1`.
- [ ] Every model entry has exactly `id`, `pack`, `category`, `file` — all non-empty strings.
- [ ] Every `file` ends in `.gltf` and is a bare filename (no slashes).
- [ ] Every `id` is unique across the whole `assets` array.
- [ ] ids are **deterministic** — re-running your converter yields identical ids.

**Cross-consistency**

- [ ] Every `pack` value has a matching `pack.json` and `library/<pack>/` directory.
- [ ] Shared atlases are referenced by URI, not duplicated per asset.

If all boxes are checked, the library will ingest cleanly.

---

## 7. Worked micro-example

A two-asset, single-pack library:

```
my-library/
├── _library_config/
│   └── catalog.json
└── library/
    └── medieval_village/
        ├── pack.json
        ├── textures/
        │   └── Village_Atlas.png
        ├── buildings/
        │   ├── House_01.gltf      (images[].uri = "../textures/Village_Atlas.png",
        │   │                        buffers[].uri = "House_01.bin")
        │   └── House_01.bin
        └── props/
            ├── Barrel_01.gltf     (images[].uri = "../textures/Village_Atlas.png",
            │                        buffers[].uri = "Barrel_01.bin")
            └── Barrel_01.bin
```

`library/medieval_village/pack.json`:

```json
{ "slug": "medieval_village", "name": "Medieval Village", "color": "8a5a2bff" }
```

`_library_config/catalog.json`:

```json
{
  "schemaVersion": 1,
  "assets": [
    { "id": "uid://mv_house_01", "pack": "medieval_village", "category": "buildings", "file": "House_01.gltf" },
    { "id": "uid://mv_barrel_01", "pack": "medieval_village", "category": "props",     "file": "Barrel_01.gltf" }
  ]
}
```

Both assets share one atlas (referenced, not copied). Toybox reads each glTF, finds the
shared texture and the sibling `.bin`, derives the names `House_01` / `Barrel_01`, records
that neither is animated, and presents them under the **Medieval Village** pack in the
**buildings** / **props** categories. Done.

---

## 8. Conversion strategy (recommended agent workflow)

1. **Survey the source.** Inventory every model, its format, its textures, and any
   existing stable identifiers (GUIDs/uids/hashes). Note the source's own taxonomy.
2. **Decide pack(s) and category slugs.** Map the source taxonomy onto
   `<pack>`/`<category>` slugs. Keep the category vocabulary small and consistent.
3. **Convert each model to separate-file glTF 2.0** with external textures, writing it to
   `library/<pack>/<category>/`. Ensure texture URIs are relative and resolve. Prefer a
   shared `library/<pack>/textures/` atlas folder referenced via `../textures/...`.
4. **Write one `pack.json` per pack.**
5. **Mint stable ids** deterministically (from the source id if present, else a hash of
   `pack/category/file`).
6. **Emit `catalog.json`** — one four-field entry per converted glTF.
7. **Run the §6 checklist.** Fix every miss. Re-run your converter to confirm ids and
   output are deterministic.

The output is a self-contained library directory; point Toybox at its root to ingest.
On that first open Toybox converts `catalog.json` into `library.db`; afterwards, change
the library only through Toybox.

## Deleting library items

Right-click a tile (or a tile of the selection) and use **Delete**. After confirmation, Toybox
removes their rows (including variants) from `library.db` (the items, their derived
asset rows, tags and rig links), and deletes their glTF, binary and texture files.
Files referenced by surviving items or their variants are kept. Rescanning does not
restore deleted items. Other items keep their metadata; preview-model
preferences pointing at removed items are cleared.

The operation stops and drains thumbnail generation first. Before the rows go,
Toybox writes `catalog-before-delete-<timestamp>.json` files holding the deleted
entries (in `_library_config/`) and the deleted derived asset rows (beside the
per-machine `user.db`); the newest few are kept. The rows are removed in one
database transaction, which also records the deletion in the change log, and only
the rows of the deleted items are read or written. Exclusive asset files are
validated against the canonical library data directory,
deduplicated and staged by renaming before the transaction commits. A failed prepare
or database write restores those files. On success the exact staged files are
erased; no folder is recursively deleted. The backups contain metadata only,
and cannot recover erased asset data.
## Import provenance and tags

Every manifest entry can carry optional `provenance` metadata, independent of
its physical pack/category path:

```json
"provenance": {
  "filesystemPath": "D:/downloads/Space Pack.zip",
  "source": "Space Pack",
  "publisher": "Example Studio",
  "url": "https://example.test/space-pack",
  "license": "CC0"
}
```

The host import screen applies these fields to the batch, including variants.
Both directory-picker APIs fill the filesystem origin unless the user has
entered a different value. Plugins can also supply per-entry provenance.
Whitespace-only fields are omitted; omitted provenance on reimport retains the
prior stored value. Extracted animation entries inherit source provenance.

Asset records expose it as `user.provenance` (the filesystem path is kept in the
per-machine `user.db`; the other fields are in the entry), alongside generated tags
`source: Space Pack`, `publisher: Example Studio`, and `license: CC0`. These
tags support text search and exact tag filters. Changing declared provenance
replaces its previous generated tags while retaining unrelated user tags,
favorites, and collections. Original paths and URLs remain metadata, not tags
or instructions to relocate files. The metadata is recorded as supplied; no
license is inferred when the field is blank.
