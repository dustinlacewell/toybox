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

Migration version 6 (`substance-assets`) makes every Substance archive a material keeps as a
shared source (`library/materials/_sources/<sha256>/<name>.sbsar`) an asset of its own, a
*substance* (plugin type `substance`), and links its materials to it with `origin.derivedFrom`.
The journal is advanced to 6 first (after a snapshot of `library.db` when anything will be
converted; see "Going back from step 6" below). Then, one archive at a time, with a progress line
on standard error (`toybox: Substance archive <n>/<total>: <name> (<MB>)`; never on standard
output, so `tbx --json` output is unaffected):

1. The substance entry is planned: id `uid://substance_<first 32 hex of the sha256>`, name the
   one material's when only one material (or variant) uses the archive, otherwise the archive's
   file stem; category, tags and provenance of the first material; `origin.plugin` of the
   materials; `format: "sbsar"`; `meta: {sbsar, sha256, bytes}`; files `[<archive>,
   "preview.png"]`, the preview a square downscale (at most 512 pixels) of the first
   material's albedo map, left out when that map cannot be read. Its folder
   `library/substance/<key>/` is computed by the same code a normal commit uses
   (`storage::planned_folder`), so the same archive imported later lands in the same folder.
2. `_library_config/substance-pending.json` records where the archive is, where it goes, the
   files the step creates and the substance's storage path. It is never written while one is
   there: an unsettled one stops the run.
3. The preview is written and the archive gets a **second name** in the substance's folder: a
   hard link (same volume, no copy, no extra space, so a library on a nearly full disk
   converts). The `_sources` name stays. Where hard links are not supported it is renamed
   instead (only when nothing else lists it); where that fails too it is copied.
4. The archive is read once (streaming sha256) and compared with the name of its `_sources`
   folder. On a mismatch the group is undone and skipped: the archive stays, its materials are
   untouched, and no substance records a false content hash. This is the one read of the
   archives (minutes for 40 GB); a copy is verified by that same single read.
5. One journaled publication puts the substance and rewrites the materials (and variants) in
   one database transaction: the archive's path leaves `origin.files` (an empty list goes),
   `origin.derivedFrom` is set, `format: "sbsar"` is added where missing. Ids, folders, maps,
   tags, personal data, thumbnails and content hashes of the materials do not change.
6. The `_sources` name is removed (when a scan of the entries under the same lock proves that
   nothing else lists it), then the pending file and the emptied `_sources/<sha256>/` folder.
   A failure here is a warning in the report, not a skip: the leftover is only another name of
   the substance's archive (or a verified copy), and `tbx library check --fix` may remove it.

A material that lists two archives is linked to the first one in `origin.files` order; the
others become substances too and leave its `origin.files`, and the report lists each such
material (`extraLinks`).

Invariants:

* Every path an entry names is there at every moment: before the commit the entries name the
  `_sources` path, which a hard link leaves in place; after it the substance names its folder,
  where the archive was put before the commit. So a rescan or `library check` never finds a
  missing source because of this step (except, on a file system without hard links, between
  the rename and the commit or its rollback).
* The substance and the material rewrites are one transaction.
* The step never decides from the database while a storage publication journal
  (`storage-pending-items.json`) could still be replayed: a commit that fails after writing its
  journal (a full disk refusing the database write, say) has that journal finished at once; if
  that fails too, the run stops with both journals in place. The next open finishes the
  publication first (`storage::recover`) and only then settles the step's pending file. A
  publication is also never started while an older publication journal exists (it would be
  overwritten).
* Settling the pending file: if the substance is stored with the recorded storage path, the
  `_sources` name is removed when nothing else lists it; otherwise what the step added is
  removed (the link, the copy, the preview; a renamed archive is renamed back) and the archive
  is converted again. If settling fails (a rename back refused by a file in use), the run
  stops with the pending file in place; the library still opens (the failure is logged) and
  the next open tries again.

| State when a run stops | What the next open does |
| --- | --- |
| Pending file written, nothing placed yet | Removes what is there of the preview, drops the pending file, converts the archive again. |
| Archive linked (or copied) into the substance folder, not committed | Removes the link or copy and the preview (the `_sources` name never left), converts again. |
| Archive renamed (no hard links), not committed | Renames it back to `_sources` (or links/copies it back), converts again. If that fails: logged, the next open tries again. |
| Commit failed, its publication journal left | `storage::recover` finishes the publication first (the substance is committed); then the pending file is settled as committed. If the publication cannot be finished the open fails as for any unfinished publication (nothing is undone). |
| Committed, `_sources` name not removed yet | Removes the `_sources` name when nothing lists it, drops the pending file. |
| Committed, cleanup failed (warning) | Nothing to do: the extra `_sources` name is an orphan `check --fix` may remove. |

Run order on open: an interrupted publication is finished, then step 5's leftovers, then
step 6's pending file, then the archives left. A second process finds the storage lock taken
and leaves the work.

An archive whose substance exists already (an older client committed an old-style material
through a newer server, or imported the same archive again) only has its materials linked; the
duplicate shared copy is removed after checking it has the same content and nothing lists it.
An import that replaces the only material listing an archive whose substance does not exist
yet with a material derived from that substance converts the archive first, so the import never
removes the only copy. An archive that cannot be converted (missing, not what its name says,
not placeable) stays with its materials, is listed in the report (`tbx library info`), and is
remembered with its size and modification time: it is tried again when those change or another
Toybox version opens the library, never on every open or write. Checking whether anything is
left costs `meta` reads while nothing was written (plus a `stat` per remembered skip), and
one range of the file-reference index after a write; the entries are read only when that index
finds an archive that was not tried in its current state (or after the derived rows were
rebuilt). Materials whose kept sources are not Substance archives, and models, are not touched:
`_sources/` remains the general place for kept sources.

### Going back from step 6

Before step 6 changes anything, a snapshot of `library.db` is taken
(`_library_config/snapshots/library-v5-<UTC>.db`). It is a schema 1 database at format 5, the
one Toybox 0.5.x reads (this build upgrades `library.db` to schema 2 on any open, so the
snapshot is brought back down; schema 2 only added a derived column). What restoring it gives
back: the entries, derived rows and content hashes as they were before step 6, and the journal
put back to 5. What it does not: the files. Once step 6 ran, each converted archive is in
`library/substance/<key>/` and no longer in `_sources/`, so the restore refuses (it checks that
every file the snapshot's entries name exists) until each archive is put back at the
`_sources/<sha256>/<name>` path the snapshot's materials list (a copy or a hard link of the
substance's file). Substances, and anything imported or edited after the snapshot, are not in
it. Personal data (`user.db`) is not part of a snapshot.

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
