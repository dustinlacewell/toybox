# Managed library storage

For an operational migration procedure, see the
[AI library migration guide](ai-library-migration.md).

The library's items are authoritative. They are the rows of the `items` table of
`_library_config/library.db` (a SQLite database; `items.entry` holds each item's entry
JSON), which since migration step 4 replaces the JSON manifest
`_library_config/catalog.json`. See [SQLite library storage](sqlite-storage.md) for the
files and tables. Each entry has an explicit `storagePath`; source titles and
categories never locate files.

```
library/
  models/<opaque-set-key>/Name.gltf + Name.bin
  models/_resources/<content-hash>.<extension>
  materials/<opaque-set-key>/<map files>
  animations/<opaque-set-key>/Name.gltf + Name.bin
  audio/<opaque-set-key>/<audio file>
```

These directories manage file dependencies and collisions. They are not library
groups. Discovery, search and filters use indexed metadata and tags. Existing
IDs, names, variants, rig bindings, favorites, collections, preview-model choices,
and provenance remain attached to the same items. Favorites, personal tags,
collections and preview-model choices are personal to a computer and live in its
`user.db`, never in the library.

The `pack` field remains for compatibility with older plugins and existing
export layouts. It does not determine a managed storage location. Native seed
entries with `storagePath` can omit pack and category. Import panels expose
provenance and category; their private staging namespace is not a destination
the user must organize. The host normalizes every import before committing it.
Extracted animations also write directly into the animation type directory.

Migration version 3 copies and SHA-256 verifies referenced media before publishing
the new manifest and cache (in a library at version 4 or later, the entries and
derived rows in `library.db`; the publication is one database transaction). It rewrites glTF resource URIs and cached media
paths, without reindexing, extracting animations, or inferring new metadata.
Shared model textures deduplicate by content, and same-named files from different
sources cannot overwrite one another. Original path-derived animation identities
are retained in glTF extras for later extraction.

`_library_config/storage-pending.json` records publication and cleanup so an
interrupted migration resumes safely. External edits or altered destinations
stop recovery before originals are removed. Migration backups retain previous
manifest/cache documents (after step 4, `items.before.json`: only the entries of the items it changes), original scene JSON, and a transaction file mapping
original media paths to verified destinations. Unindexed files and pack metadata
from old source trees are archived there instead of being classified or deleted.
Known old source directories therefore leave the managed `library/` tree.

Migration version 4 (`sqlite-store`) is the last step. It converts the JSON manifest,
the per-machine cache `libraries/<id>/catalog.json` and `content-hashes.json` into
`library.db` and `user.db`, and moves the old files, never deleting them, into
`_library_config/migration-backups/0004-<stamp>/`. See
[SQLite library storage](sqlite-storage.md#migration-library-migration-step-4-sqlite-store).

The app runs migrations when opening a library. For an offline migration without
launching the UI or deriving catalog facts:

```
cargo run --bin migrate-library -- <library-root> <catalog-cache-path>
```

`<catalog-cache-path>` is still the old location of the cache file
(`<app-data>/libraries/<id>/catalog.json`): the tool reads a leftover cache there
and puts `user.db` beside it.

An older Toybox refuses a library whose migration version it cannot understand.
If Windows blocks archiving a source folder that is open in Explorer, close that
folder window and retry. The journal resumes the completed migration's cleanup.
