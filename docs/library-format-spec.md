# Toybox Asset Library Format — Conversion Spec for LLM Agents

You are converting a **foreign 3D-asset collection** (some other library, a bundle of
asset packs, a pile of loose models — anything) into the **Toybox library format**. This
document is the complete contract. You do not need access to the Toybox application or its
source. Produce a directory tree that matches what is described here and Toybox will ingest
it.

Read the whole document once before starting. The format is small; the discipline is in
getting paths and the source manifest exactly right.

---

## 1. What you're producing — the mental model

A Toybox library is a **directory** with two parts:

```
<library-root>/
├── _library_config/
│   └── catalog.json          ← the manifest YOU author (the only hand-written file)
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
`material-set-format.md`. The rest of this document covers models.

Everything else — each asset's texture set, its `.bin`, its animation clips, thumbnails,
user tags — Toybox computes or owns. **Do not try to author those.** Your job ends at the
three things above.

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
Toybox seeds from. Shape:

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
  `1`.)
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

- [ ] `<library-root>/_library_config/catalog.json` exists and is valid JSON.
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
