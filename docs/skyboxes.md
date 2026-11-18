# Skyboxes

Toybox stores skies as `skybox` assets. The type is defined entirely by the
bundled **Skyboxes** plugin (`plugins/com.toybox.skybox-core`): its declaration,
importer, viewer, thumbnails and properties. See [Asset types](asset-types.md)
for the contract it is built on.

## What can be imported

| Sky | Files | How it is recognised |
| --- | --- | --- |
| Panorama (equirectangular) | one HDR, EXR, PNG, JPG or WebP | the image is 2:1 |
| Cube map, six images | six files in one folder | names end in the six faces |
| Cube map, horizontal cross | one image | 4:3 |
| Cube map, vertical cross | one image | 3:4 |
| Cube map, strip | one image | 6:1 or 1:6 |

HDR (Radiance) and EXR keep their full light range; they are tone mapped only
for display. Files are copied as they are, never converted.

Six images form a cube map when they share a folder, a name and an extension
and each ends in a different face, in any of these conventions:

| +X | −X | +Y | −Y | +Z | −Z |
| --- | --- | --- | --- | --- | --- |
| `px` | `nx` | `py` | `ny` | `pz` | `nz` |
| `posx` | `negx` | `posy` | `negy` | `posz` | `negz` |
| `+x` | `-x` | `+y` | `-y` | `+z` | `-z` |
| `right`, `rt` | `left`, `lf` | `top`, `up` | `bottom`, `down`, `dn` | `front`, `ft` | `back`, `bk` |

The face follows a separator or is the whole name: `sky_px.png`, `Left.jpg`,
`night-up.hdr`. A set with a face missing or named twice is listed as separate
images instead.

Crosses use the common layouts: top above and bottom below the front face, with
the back face at the far right (horizontal) or below the bottom, upside down
(vertical). Strips run +X, −X, +Y, −Y, +Z, −Z.

Not supported: KTX2 and DDS cube maps, and cube maps stored as six faces in
other layouts (for example a 3 × 2 grid).

## Importing

Choose **Skyboxes → Import**, pick the **Skyboxes** plugin and a folder. The
folder is scanned recursively. HDR and EXR images, cube maps, and images whose
name says they are a sky (`sky`, `pano`, `hdri`, `env`, `cubemap`, …) are
ticked; other images are listed unticked, so a folder of mixed textures does
not import them by accident.

Select a row to preview it — drag to look around. For a single image the
**Layout** is read from its shape and can be overridden; an image that is not
2:1 is shown as a panorama with a note to check it.

Importing a folder again updates its skies in place: ids are derived from the
source folder and the sky's name.

## Viewing

Open a sky to look around inside it: drag (or use the arrow keys) to turn,
scroll (or `+` / `−`) to zoom. The **Skybox** tab has:

- **Exposure**, in stops, for inspecting bright and dark regions of an HDR.
- **Show lighting spheres**: a mirror and a matte sphere lit by the sky.

Library thumbnails are a still from inside the sky, looking just above the
horizon. Large panoramas are shrunk for the thumbnail only.

## Exporting

**Copy files** exports the original files: a panorama as `<name>.<ext>`, a
six-image cube map as a `<name>/` folder with its face files.

## Manifest entry

```json
{ "kind": "plugin", "type": "skybox", "id": "uid://skybox_2a0faf8624772c7e", "pack": "imported",
  "category": "skies", "name": "lobby",
  "files": ["lobby_px.png", "lobby_nx.png", "lobby_py.png", "lobby_ny.png", "lobby_pz.png", "lobby_nz.png"],
  "meta": { "layout": "cubemap", "encoding": "ldr", "format": "PNG", "width": 1024, "height": 1024,
            "faces": { "px": "lobby_px.png", "nx": "lobby_nx.png", "py": "lobby_py.png",
                       "ny": "lobby_ny.png", "pz": "lobby_pz.png", "nz": "lobby_nz.png" } },
  "storagePath": "library/skybox/<key>" }
```

`meta.layout` is `equirect`, `cubemap`, `cross-horizontal`, `cross-vertical`,
`strip-horizontal` or `strip-vertical`; `meta.encoding` is `hdr`, `exr` or
`ldr`; `width` and `height` are the image's pixels (one face's, for six
images). Missing or damaged metadata is rebuilt from the file names.
