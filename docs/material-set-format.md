# Toybox Material Set Format

> The pack-folder examples below describe legacy inbound material entries.
> Managed storage in 0.4.1 uses an explicit `storagePath` under `library/materials/`.
> Follow the [AI migration guide](ai-library-migration.md) for existing libraries
> and the [storage overview](library-storage.md) for the current layout.

A material set is a group of texture maps. Each map has a role, such as albedo or normal.
A material set is not a glTF model. It has its own entry and its own folder. (An
entry is the JSON an importer produces. Toybox stores it in the library database
`_library_config/library.db`, in `items.entry`; before migration step 4 it was an
element of `assets[]` in `_library_config/catalog.json`. The entry shape is the same.
See [SQLite library storage](sqlite-storage.md).)

This document describes material entries. For models, packs and the rest of the library
layout, read `library-format-spec.md`.

---

## 1. Folder layout

```
library/<pack>/materials/<name>/<file>
```

- `<pack>` is the pack slug. It must match a `pack.json` `slug`.
- `materials` is a fixed folder name. The category is **not** part of the path.
- `<name>` is the material `name` from the manifest entry.
- `<file>` is a texture file. Each map in the entry names one file in this folder.

Because `materials` is fixed, a model category must not be named `materials`.

Example:

```
library/rock_pack/materials/granite_01/
├── granite_01_albedo.png
├── granite_01_normal.png
└── granite_01_orm.png
```

---

## 2. Manifest entry

A material entry is one item of the library's manifest, next to model entries: a row of
the `items` table of `_library_config/library.db` (`items.entry` holds this JSON). In a
library from before migration step 4 it sat in `assets[]` of
`_library_config/catalog.json`, and the same entry shape is still what an importer
hands to Toybox. Do not edit the database by hand; entries are added and changed
through Toybox (the importer commit and the metadata commands). The field `kind` tells
the two apart.

| Field            | Type    | Required | Meaning                                                      |
| ---------------- | ------- | -------- | ------------------------------------------------------------ |
| `id`             | string  | yes      | Stable, globally unique key. Same rules as for models.       |
| `kind`           | string  | yes      | Must be `"material"`.                                        |
| `pack`           | string  | yes      | Pack slug.                                                   |
| `category`       | string  | yes      | Category slug. Used for filtering. Not part of the path.     |
| `name`           | string  | yes      | Display name. Also the folder name under `materials/`.       |
| `maps`           | array   | yes      | The maps of the material. See section 3.                     |
| `flipNormalGreen`| boolean | no       | `true` if the normal map uses the DirectX convention.        |

`kind` is an optional field on every entry. If `kind` is absent or `"model"`, the
entry is a glTF model. If `kind` is `"material"`, the entry is a material set. Any other
value is an error.

---

## 3. Maps

Each item of `maps` has these fields:

| Field     | Type    | Required | Meaning                                                  |
| --------- | ------- | -------- | -------------------------------------------------------- |
| `role`    | string  | yes      | What the map is for. See the role list below.            |
| `file`    | string  | yes      | Bare file name in the material folder. No path.          |
| `channel` | string  | no       | `r`, `g`, `b` or `a`. The channel the role reads.        |
| `invert`  | boolean | no       | `true` if the stored values are inverted.                |

### Roles

- `albedo` — base color.
- `normal` — surface normals.
- `roughness`
- `metallic`
- `ao` — ambient occlusion.
- `height`
- `emissive`
- `opacity`

### Channels

If `channel` is absent, the role reads the whole image. If `channel` is present, the role
reads that one channel of the file. Use `channel` for scalar roles in a packed texture.

Several roles may name the same file with different channels. For example, an ORM file
holds ao in `r`, roughness in `g` and metallic in `b`.

### Flags

- `invert` marks a scalar role that the file stores inverted. For example, a file holds
  smoothness and the role is `roughness`. Set `invert` to `true`.
- `flipNormalGreen` marks a normal map in the DirectX convention. It sits on the entry,
  not on a map.

---

## 4. Rules

- The entry must have exactly one `albedo` map.
- A role may appear once in `maps`.
- Each `file` must be a bare file name that exists in `library/<pack>/materials/<name>/`.
- `id` must be unique across the whole catalog and stable across re-imports.

The scan checks that each named file exists. It does not open the files. It does not check
that a file is a valid image or that a channel exists.

---

## 5. Examples

A material with a packed ORM file:

```json
{
  "id": "uid://rk_granite_01",
  "kind": "material",
  "pack": "rock_pack",
  "category": "stone",
  "name": "granite_01",
  "flipNormalGreen": true,
  "maps": [
    { "role": "albedo",    "file": "granite_01_albedo.png" },
    { "role": "normal",    "file": "granite_01_normal.png" },
    { "role": "ao",        "file": "granite_01_orm.png", "channel": "r" },
    { "role": "roughness", "file": "granite_01_orm.png", "channel": "g" },
    { "role": "metallic",  "file": "granite_01_orm.png", "channel": "b" }
  ]
}
```

A minimal material with an albedo map only:

```json
{
  "id": "uid://rk_sand_01",
  "kind": "material",
  "pack": "rock_pack",
  "category": "ground",
  "name": "sand_01",
  "maps": [
    { "role": "albedo", "file": "sand_01_albedo.png" }
  ]
}
```
