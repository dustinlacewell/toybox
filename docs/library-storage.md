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
  materials/_sources/<sha256>/<original name>   (kept source files: one copy per content)
  models/_sources/<sha256>/<original name>
  animations/<opaque-set-key>/Name.gltf + Name.bin
  audio/<opaque-set-key>/<audio file>
```

These directories manage file dependencies and collisions. They are not library
groups. Discovery, search and filters use indexed metadata and tags. Existing
IDs, names, variants, rig bindings, favorites, collections, preview-model choices,
and provenance remain attached to the same items. Favorites, personal tags,
collections and preview-model choices are personal to a computer and live in its
`user.db`, never in the library.

Kept source files (`origin.files`) live apart from the items that keep them. Each
distinct file is stored **once per library**, by content, in
`library/<type>/_sources/<sha256>/<original name>` (`<type>` is `materials` or `models`,
the types that can keep sources). An entry names that library-relative path in
`origin.files`, in the manifest and in the catalog alike; the last segment is the
file's original name, which is how a plugin finds "its" file (the Substance plugin
looks for `origin.meta.sbsar`). An importer still stages a bare name next to the entry's
own files; the host moves the staged file into this folder when the entry is
committed, or just drops it when a file with that content is stored already. A source
is deleted with the last item that lists it (the file-reference table says who does),
and a replaced item gives up the ones nothing else lists. An older entry may still list
a bare name that sits in its own folder until the library is converted
(migration step 5, below).

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

Migration version 5 (`shared-sources`) gives every kept source file its place in
`_sources/` (see "Kept source files" above). The library journal is advanced to version 5
first, so an older Toybox refuses the library from then on. Then, in batches of items
through the same journaled publication an import uses, the step hashes each source beside
its item, writes the first file of each content into `_sources/<sha256>/` (verified by size
and hash), rewrites the entries and the derived rows in one database transaction, and only
then removes the copies beside the items, each only if it still is what was hashed. A crash
at any point leaves a valid library: before the transaction the entries still name the old
files, after it the shared ones; an interrupted publication is finished from its journal at
the next open, and the step itself (`meta` key `sharedSources` is `done` when finished) resumes
at the next open. An item whose source is missing or unreadable is left as it was and listed in
the report (`meta` key `sharedSourcesReport`: items converted, files, bytes before, bytes freed,
items skipped), which `tbx library info` shows. The step runs wherever a library is opened:
the app, `tbx`, and the server at start. It needs time proportional to the bytes of the sources
(every one is hashed twice) and no space beyond the first file of each content.

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
