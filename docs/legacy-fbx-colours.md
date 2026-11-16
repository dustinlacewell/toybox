# Legacy FBX vertex-colour repair

Some converted opaque FBX models have predominantly zero RGBA vertex colours
used as source shader masks. glTF multiplies these into base colour, producing
black surfaces while emissive remains visible.

Toybox repairs only textured, opaque primitives whose material explicitly has
`extras.fromFBX.isTruePBR: false` and whose VEC4 colour accessor is more than half
all-zero RGBA. It removes that primitive's `COLOR_0` binding. UVs, material
assignments, texture references, geometry, and binary data remain intact. Authored
PBR, transparent materials, RGB colours and opaque black RGBA are retained.

The normalization is shared by previews and native exports, and applied when
promoting newly converted FBX files. It does not choose a replacement atlas by
filename: shared atlases often include both character palettes and buildings.

For existing source files, `cargo run --bin repair-colors -- <library-root>`
reports matches. Add `--apply` to write corrections with mirrored originals and
a report under `_library_config/vertex-color-backup-<timestamp>/`. The operation
is idempotent. Only thumbnails for affected source files need regeneration.
