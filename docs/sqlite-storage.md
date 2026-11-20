# SQLite library storage

Status: built through step 5 (the 1,000,000 item benchmark; results at the end of "App queries"). Goal: a library of 100,000 to 1,000,000 items stays smooth to
browse, edit, import and sync. Cost of an operation must depend on what it touches,
not on how large the library is.

## Why

Before this change a library was two JSON documents: the manifest
`_library_config/catalog.json` (the truth) and a derived per-machine cache
`catalog.json` (plus `content-hashes.json`). Every import, tag edit, delete and
server ingest read, re-validated and rewrote both in full, and every client loaded
the whole catalog into memory. At 12,000 items a server ingest of 100 models took
24 to 60 seconds and the catalog body was 53 MB.

## Decisions

- SQLite is the source of truth for a library. No sidecar metadata files.
- Asset files stay on disk exactly where they are (`library/<type>/<hash>/...`,
  shared images in `_resources/`). Thumbnails stay PNG files on disk.
- Personal data (favourites, personal tags, collections, preview choice, the path
  an item was imported from) never enters the library database or a server.
- The JSON shapes on the wire and in the app (`Catalog`, `Asset`, manifest entry)
  do not change. Storage changes underneath them.

## Files

| File | Where | Holds |
|---|---|---|
| `library.db` | local library: `<library>/_library_config/`; remote library: `<app-data>/libraries/<id>/` (a synced replica) | items, tags, provenance, derived catalog rows, content hashes, rigs, revision log |
| `user.db` | `<app-data>/libraries/<id>/` | favourites, personal tags, collections, preview choice, import path, thumbnail error |
| `browse-index.db` | `<app-data>/` | derived and per machine: the merged list order and the copy marks of all libraries (see "The browse index"). Safe to delete; rebuilt |

Both use WAL mode, `synchronous=NORMAL`, `foreign_keys=ON`, and a `user_version`
for their own schema. One writer at a time per library is still enforced by
`_library_config/storage.lock` plus the in-process mutex.

## `library.db` schema (version 1)

```sql
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- keys: storageVersion, manifestSchemaVersion, revision (monotonic integer),
--       manifestRevision (revision of the last manifest change), derivedFor (the
--       manifestRevision the asset rows were derived for), catalogSchema,
--       extra (JSON object of unknown top-level manifest keys), topOrder, hasManifest,
--       dbId, hashScheme, hashRevision, personalTagsMigrated,
--       changesFloor (the newest revision whose tombstones were pruned),
--       thumbRevision (the shared-thumbnail log, apart from revision);
--       a replica also keeps its cursor: remoteRevision, remoteAfter, remoteDbId,
--       remoteThumbRevision

CREATE TABLE items (
  id            TEXT PRIMARY KEY,          -- top-level item id
  seq           INTEGER NOT NULL UNIQUE,   -- manifest order
  kind          TEXT NOT NULL,             -- model | material | animation | audio | plugin
  type          TEXT NOT NULL,             -- kind, or the plugin type id
  pack          TEXT NOT NULL,
  category      TEXT NOT NULL,
  name          TEXT NOT NULL,
  storage_path  TEXT,
  entry         TEXT NOT NULL,             -- the manifest entry, verbatim JSON (variants inline)
  revision      INTEGER NOT NULL,          -- revision that last changed this row
  pkey          TEXT                       -- physical key "pack<US>kind<US>file" (NULL when it has none)
);
CREATE INDEX items_pkey ON items(pkey);                 -- who an incoming entry absorbs
CREATE INDEX items_storage ON items(storage_path);

CREATE TABLE item_variants (               -- ids of the variants inside each entry
  variant_id TEXT NOT NULL,
  item_id    TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  thumb_name TEXT NOT NULL,                -- sanitised id: the thumbnail file name
  PRIMARY KEY (variant_id, item_id)
) WITHOUT ROWID;                           -- "which item owns this id" is one lookup
CREATE INDEX item_variants_item ON item_variants(item_id);
CREATE INDEX item_variants_thumb ON item_variants(thumb_name);

CREATE TABLE assets (                      -- derived catalog rows (what the app shows)
  id            TEXT PRIMARY KEY,          -- catalog asset id (a clip row has its own)
  item_id       TEXT,                      -- the manifest item it is derived from (NULL in a replica)
  ord           INTEGER NOT NULL,          -- catalog order
  kind          TEXT NOT NULL,
  type          TEXT NOT NULL,
  name          TEXT NOT NULL,
  pack          TEXT NOT NULL,
  category      TEXT NOT NULL,
  format        TEXT NOT NULL DEFAULT '',
  source        TEXT NOT NULL DEFAULT '',
  publisher     TEXT NOT NULL DEFAULT '',
  license       TEXT NOT NULL DEFAULT '',
  content_hash  TEXT,
  hash_key      TEXT,                      -- "<folder>/<key>[#clip]" the hash belongs to
  asset         TEXT NOT NULL,             -- Asset JSON without user, thumb, contentHash
  revision      INTEGER NOT NULL,
  rel_path      TEXT NOT NULL DEFAULT '',  -- the asset's rel_path (what grouping matches on)
  thumb_name    TEXT NOT NULL DEFAULT '',  -- sanitised id: the thumbnail file name
  clip_source   TEXT NOT NULL DEFAULT '',    -- for a clip, the id of the model it was cut from
  plugin        TEXT NOT NULL DEFAULT ''     -- the plugin that imported it (origin.plugin)
);
CREATE INDEX assets_item ON assets(item_id, ord);
CREATE INDEX assets_order ON assets(ord);
CREATE INDEX assets_rel ON assets(rel_path);
CREATE INDEX assets_thumb ON assets(thumb_name);
CREATE INDEX assets_clip_source ON assets(clip_source);
CREATE INDEX assets_browse ON assets(type, name COLLATE NOCASE, id, content_hash);   -- the list, in order, with the hash for the copy probe
CREATE INDEX assets_hash ON assets(type, content_hash);
CREATE INDEX assets_unhashed ON assets(type, id) WHERE content_hash IS NULL;         -- the few items without a hash yet
CREATE INDEX assets_category ON assets(type, category, name COLLATE NOCASE, id);
CREATE INDEX assets_source ON assets(type, source);
CREATE INDEX assets_publisher ON assets(type, publisher);
CREATE INDEX assets_license ON assets(type, license);
CREATE INDEX assets_format ON assets(type, format);
CREATE INDEX assets_hash_key ON assets(hash_key);
CREATE INDEX assets_revision ON assets(revision);

CREATE TABLE asset_tags (                  -- library tags, one row each
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  tag      TEXT NOT NULL,
  type     TEXT NOT NULL DEFAULT '',        -- the asset's type, so the counts need no join
  PRIMARY KEY (asset_id, tag)
) WITHOUT ROWID;
CREATE INDEX asset_tags_tag ON asset_tags(tag, asset_id);

CREATE TABLE asset_files (                 -- every library file an asset (and its variants) lists
  path     TEXT NOT NULL COLLATE NOCASE,
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  origin   INTEGER NOT NULL DEFAULT 0,     -- 1: a kept source file, not content
  PRIMARY KEY (path, asset_id)
) WITHOUT ROWID;                           -- "which files only these assets own" is one query
CREATE INDEX asset_files_asset ON asset_files(asset_id);

CREATE VIRTUAL TABLE asset_search USING fts5(name, tags, content='', contentless_delete=1, tokenize='trigram');

CREATE TABLE rigs (id TEXT PRIMARY KEY, rig TEXT NOT NULL, kind TEXT NOT NULL DEFAULT '', shape TEXT NOT NULL DEFAULT '');
CREATE INDEX rigs_shape ON rigs(shape);    -- candidates for "is this rig equivalent to a stored one"
CREATE TABLE rig_uses (                    -- which assets bind to which rig
  rig_id   TEXT NOT NULL,
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  PRIMARY KEY (rig_id, asset_id)
) WITHOUT ROWID;
CREATE INDEX rig_uses_asset ON rig_uses(asset_id);

CREATE TABLE changes (                     -- what a client needs to catch up
  revision INTEGER NOT NULL,
  asset_id TEXT NOT NULL,
  deleted  INTEGER NOT NULL,
  PRIMARY KEY (asset_id)                   -- only the latest change per asset is kept
) WITHOUT ROWID;
CREATE INDEX changes_revision ON changes(revision, deleted, asset_id);   -- /changes pages read this
CREATE INDEX changes_tombstones ON changes(revision) WHERE deleted = 1;  -- pruning reads this

CREATE TABLE asset_hash_keys (             -- the storage keys whose content hash an asset (or a variant) shows
  hash_key TEXT NOT NULL,
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  hashed   INTEGER NOT NULL DEFAULT 0,     -- the key has a hash in `hashes`
  PRIMARY KEY (hash_key, asset_id)
) WITHOUT ROWID;                           -- a hash that arrives touches exactly these rows
CREATE INDEX asset_hash_keys_asset ON asset_hash_keys(asset_id);
CREATE INDEX asset_hash_keys_pending ON asset_hash_keys(asset_id) WHERE hashed = 0;   -- "what still needs hashing"

CREATE TABLE thumbs (                      -- shared pictures saved through the server, by thumbRevision
  asset_id TEXT PRIMARY KEY,
  revision INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX thumbs_revision ON thumbs(revision);

CREATE TABLE hashes (key TEXT PRIMARY KEY, hash TEXT NOT NULL) WITHOUT ROWID;   -- content hashes by storage key

CREATE TABLE facets (                      -- item counts, kept by triggers on assets and asset_tags
  type TEXT NOT NULL, field TEXT NOT NULL,  -- field: type | category | source | publisher | license | format | tag
  value TEXT NOT NULL, n INTEGER NOT NULL,
  PRIMARY KEY (type, field, value)
) WITHOUT ROWID;
CREATE INDEX facets_top ON facets(type, field, n DESC, value);
```

`revision` in `meta` goes up by one per committed write. It replaces the old
content-hash ETag: "has anything changed" is one integer comparison, and
"what changed since N" is `SELECT ... FROM changes WHERE revision > N` (the
revision feed, below). A content hash that arrives for rows that show it is such a
write too (see "Rules for writes").

## `user.db` schema (version 1)

```sql
CREATE TABLE user_assets (
  asset_id        TEXT PRIMARY KEY,
  favorite        INTEGER NOT NULL DEFAULT 0,
  preview_asset   TEXT,
  filesystem_path TEXT,
  thumb_error     TEXT,
  provenance      TEXT                     -- JSON, only fields the library does not hold
);
CREATE TABLE user_tags (asset_id TEXT NOT NULL, tag TEXT NOT NULL, ord INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (asset_id, tag)) WITHOUT ROWID;
CREATE TABLE user_collections (asset_id TEXT NOT NULL, name TEXT NOT NULL, ord INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (asset_id, name)) WITHOUT ROWID;
CREATE INDEX user_collections_name ON user_collections(name, asset_id);
CREATE INDEX user_assets_preview ON user_assets(preview_asset);   -- who chose this asset as a preview
CREATE INDEX user_tags_tag ON user_tags(tag, asset_id);           -- items that carry a personal tag
CREATE INDEX user_assets_favorite ON user_assets(asset_id) WHERE favorite = 1;
```

## Rules for writes

- A commit touches only the rows of the items it changes, in one transaction that
  also bumps `revision` and records `changes`. `Store::apply(Delta)` is that
  transaction: puts (a manifest entry and the catalog assets derived from it),
  removals, hash updates. Rows equal to what is stored are not rewritten, so
  applying the same delta twice bumps nothing.
- Validation and derivation (`build_catalog`: reading a glTF, probing audio) run
  for the items in the commit only. Existing items are never read again: a
  library with a damaged old item still takes new ones.
- Everything an operation needs to know about the rest of the library is an
  indexed question: who owns an id (`items`, `item_variants`), which item a
  physical file belongs to (`items.pkey`), which assets an incoming one absorbs
  (`assets.id`, `assets.rel_path`), which files only these assets own
  (`asset_files`), which thumbnails (`thumb_name`), which rig is equivalent
  (`rigs.shape`) and who still uses it (`rig_uses`), who chose this preview
  (`user_assets.preview_asset`), which clips were cut from a model
  (`assets.clip_source`).
- Files are placed before the transaction commits (`storage::ItemPlan`); a
  failure before the journal is written removes the files this commit placed. The
  journal (`_library_config/storage-pending-items.json`) holds the delta and the
  entries as they were, i.e. only the items of the commit; recovery verifies the
  placed files, refuses if those items were edited meanwhile, applies the delta
  (a no-op if it had been applied) and retires the staged originals. The
  publication snapshot (`migration-backups/storage-<stamp>/`) holds
  `items.before.json` and the replaced scenes, not the library.
- Safety copies before destructive operations are rows-of-the-commit JSON files
  (`catalog-before-delete-*.json` holds the deleted entries, and beside the cache
  the deleted catalog assets; `catalog-before-metadata-*.json` the edited entries
  as they were), kept with the same retention counts.
- A write that needs the derived rows to be current (`Store::catalog_ready`: the
  catalog was derived for the manifest as it is, under the running schema, or the
  library is empty) is refused otherwise; the operations that write items
  (`merge_entries`, `update_entries`, `ingest_package`) first make the library ready
  (`ops::catalog::ready_for_write`: migrations, then a rescan if there is no current
  catalog). Removing items is valid in any state.
- No write path calls a whole-catalog function. `load_catalog_inner`,
  `scan_library_catalog` and `save_catalog_inner` remain for the app's whole-catalog
  reads, the version 1 `GET /catalog` (built on demand) and an explicit rescan.
- `rigs` hold a rig once, however many assets bind to it; a rig is removed when
  its last user goes (incremental deletes). A full save (`save_catalog`) keeps the
  rig list it is given.
- A content hash that arrives (`hashes::update_keys`, a `Delta`'s `hashes`) rewrites the
  rows that show it: `assets.content_hash` of the row whose own key it is, and the
  `revision` and `changes` row of every asset listed for the key in `asset_hash_keys`
  (a variant's hash belongs to its parent's row). A hash for a key no row shows moves
  only `hashRevision`. This is what carries fingerprints to clients through the feed.
- Bookkeeping that is not a library change (a thumbnail error, the migration
  marker `personalTagsMigrated`, a completed tag migration, a picture saved to the
  shared-thumbnail log) does not bump `revision`.

## Kept source files (library migration step 5, `shared-sources`)

No schema change (`user_version` stays 1): `asset_files` already lists a kept source with
`origin = 1`, and it is the file-reference table that decides what a delete removes.
What changed is the path. A source (a material's `.sbsar`, a model's original `.fbx`) is
stored once per library in `library/<type>/_sources/<sha256>/<original name>`, and the
entries of every item that keeps it, and so their `asset_files` rows, name that same path.
"Which files only these assets own" therefore keeps a shared source until the last item
that lists it goes (a source nobody lists is an orphan: `tbx library check` reports it and
`--fix` removes it). The storage key of an item no longer includes the source bytes (two
presets of one archive differ by their entries and maps); an item that is stored is never
keyed again, so no id or folder moves.

Step 5 converts a library in place: see [Managed library storage](library-storage.md). Its
state is two `meta` rows of `library.db`: `sharedSources` (`done` when finished) and
`sharedSourcesReport` (JSON: `items`, `sharedFiles`, `bytesBefore`, `bytesFreed`, `skipped`).

## Migration (library migration step 4, `sqlite-store`)

Runs on first open by a version that knows it, inside the existing migration
mechanism (`migrations.json`, `migration-backups/`, pending marker, rollback):

1. Read the manifest and the cache as they are.
2. Create `library.db` beside the manifest as `library.db.new`, insert every
   entry, derived row, tag, rig and content hash; create `user.db` from the
   cache's `user` data.
3. Verify: entry count, asset count and the id sets match; a `Catalog` rebuilt
   from the databases equals the cache (ignoring field order).
4. Rename `library.db.new` to `library.db`; move the old manifest, cache and
   `content-hashes.json` into the migration backup folder. They are not deleted.

A library without `library.db` but with `catalog.json` is still recognised as a
library (it is one that has not migrated yet). An older Toybox refuses a library
at step 4, as it already does for any newer journal.

A remote library's replica has no migration: its first sync downloads the server's feed
from 0 (a replica made before the feed existed has no cursor and is downloaded the same
way, keeping `user.db`).

## Server protocol (API version 2)

Built in step 3. The server holds no copy of the library: every answer is read from
`library.db` for the rows it needs, and a write returns what it changed. The route family
stays `/api/v1`; the **API version** (`apiVersion` of `/info`) is 2, and a request says
which shapes it wants with the header `X-Toybox-Api: 2`. A request without it (a client
from before the feed) is answered exactly as version 1 answered, so old apps keep working
against a new server (see "Compatibility").

### `GET /info`

`{ name, serverVersion, apiVersion: 2, role, revision, dbId, thumbRevision }`. With the
version 2 header `revision` is the library revision as an integer (the number `/changes`
is keyed by); without it, the string version 1 clients know (a hash of `dbId`, the catalog
schema, the revision and the hash revision: also the ETag of `/catalog`). `dbId` is the
identity of the library database, `thumbRevision` the position in the shared-thumbnail log.

### `GET /changes?since=<revision>&limit=<n>[&after=<cursor>][&dbId=<id>]`

Rows after `since`, oldest first, at most `limit` (default 1000, at most 5000):

```json
{ "dbId": "…", "reset": false, "schemaVersion": 5,
  "since": 120, "next": 134, "after": null, "revision": 134, "thumbRevision": 7, "more": false,
  "assets":  [ { "…": "each changed asset exactly as GET /catalog serves it (no personal data, contentHash set)" } ],
  "rigs":    [ { "…": "the rigs those assets bind to" } ],
  "deleted": [ "uid://gone" ] }
```

- `assets` and `deleted` are the **latest** state of every asset id changed after `since`:
  an id is in one list, never both. Variants come inside their parent.
- `revision` is the library revision when the page was read (inside one read transaction, so
  the rows and the number agree). `next` is what to ask for next: `revision` when this is
  the last page. When `more` is true the page ended inside a revision (a commit with more
  rows than `limit`) and `after` is an opaque cursor that goes with `next`: ask again with
  `since=<next>&after=<after>`.
- Within a revision, changed assets come before removed ids, each by id. A client that sees
  an asset absorb rows (a variant group formed from separate items) has the absorbed rows
  still when it learns they are gone, and can hand over their favourites and tags.
- `since=0` is a full, paged download. `limit` pages cost what they return: the query is
  `WHERE revision > ? ORDER BY revision, deleted, asset_id LIMIT ?` (or the same with the row
  value `(revision, deleted, asset_id) > (?, ?, ?)` after a cursor), a covering range scan of
  `changes_revision` (`changes_tests` checks the plan), then point reads of the page's rows.
- **`reset: true`** (with no rows, `next: 0`) means the server cannot answer for the cursor:
  `dbId` was given and is not this database (the library was replaced), `since` is beyond the
  current revision (restored from a backup), or `since` is older than `changesFloor` (below).
  The client forgets its rows (not its personal data), and starts from `since=0`. `since=0` is
  never reset. `dbId` in a reset names the database to start from.
- Errors: `400 bad_query` (`since` negative or not a number), `400 bad_cursor`.

**Retention.** `changes` keeps one row per asset id, the latest, so live rows are never
pruned: they are the current state. Tombstones (`deleted = 1`) are kept for the newest
`CHANGES_KEEP_REVISIONS` = 20,000 revisions; every commit deletes the ones older (a range
scan of the partial index `changes_tombstones`, no cost when there are none) and raises
`meta.changesFloor` to the cutoff. A client whose `since` is below the floor might have missed a
deletion and gets `reset`; one at or above it cannot have. The rule is simple on purpose: a
client that is away for more than 20,000 commits downloads the library again, which is always
correct. `Store::prune_changes(keep)` applies a smaller number (tests).

### Replies of writes

With the version 2 header, delete, metadata, commit and ingest answer with what changed and
no catalog:

| route | reply |
|---|---|
| `POST /assets/delete` | `{ revision, base, deleted: [ids asked], changes }` |
| `POST /assets/metadata` | `{ revision, base, updated: [ids asked], touched: [top-level ids], changes }` |
| `POST /uploads/{id}/commit` | `{ revision, base, imported, errors, uploadKept, changes }` |
| `POST /uploads/{id}/ingest` | `{ items, revision, base, changes }` |

`base` and `revision` are the library revision before the write began and after it ended,
read under the write lock. `changes` is the `/changes` page for `since=base` limited to 500
rows: it holds everything in `(base, revision]` (this write's rows, and fingerprints that
arrived beside it) when `more` is false. A bigger write says `more: true` and the client
pulls the rest. Handlers `tokio::spawn` the write and await its `JoinHandle`, so a write runs
to completion when the client hangs up.

### Thumbnails

Pictures are not library changes, so they have their own small log (`thumbs`,
`meta.thumbRevision`) that `PUT /thumbs/{id}` appends to. `GET /thumbs` still lists every
picture (`{ revision, thumbRevision, thumbs }`, ETag): a client does that once. `GET /thumbs?since=<thumbRevision>`
lists only the pictures saved after it. A client compares `thumbRevision` of `/changes` (or `/info`)
with the one it holds and asks only when they differ. Pictures put into the server's thumbnail
folder by anything but `PUT /thumbs` are not in the log and are noticed only by a client's next
full listing (a forced refresh).

### Compatibility

* **Old client, new server.** Without the header: `/info` carries the string revision, `/catalog`
  is the whole catalog (built on demand and kept for the revision it was built at, so a write never
  builds it; its ETag is the string revision), delete answers `{ revision, deleted, catalog }`,
  metadata `{ revision, updated, assets }`, commit and ingest carry the string revision. Only the first
  `GET /catalog` after a change pays for building it.
* **New client, old server.** An old server says `apiVersion: 1`. The client refuses it with a
  sentence ("This server is too old for this version of Toybox: it speaks version 1 of the library
  protocol and this app needs version 2...") when adding it, and shows an existing library as
  offline with that reason, serving its replica. It does not fall back to `/catalog`: a client that
  could would also have to handle the old write replies, and a library can be upgraded in a minute.
* Uploads are sent on several connections at once; ingest and commit run to completion on the
  server even if the client disconnects.

### The client

A remote library's replica (`library.db` in the app data folder, personal data in `user.db`) keeps
its cursor in its own `meta` (`remoteRevision`, `remoteAfter`, `remoteDbId`, `remoteThumbRevision`),
written in the same transaction as the page it describes, so a download that stops goes on from
the page it reached.

* **Pull.** `GET /changes` from the cursor until `more` is false. Each page is one `Store::apply`
  on the replica: the page's assets (with the personal data this computer keeps for them, read by id
  from `user.db`, and for the rows they absorb, found by id and path), the removed ids (their files and
  pictures that nothing else uses go with them, preview choices that named them are cleared), the rigs
  they bind to, and the cursor. Only these rows are read or written; applying a page twice changes
  nothing. A page with no rows and the same cursor writes nothing.
* **Start.** No cursor (first connect, a replica from before the feed, a forced refresh): the first page
  is requested from 0, then the replica's library rows are emptied (`user.db` is kept) and the pages are
  applied. `reset` does the same. A start that needs more than one page goes in **bulk mode** (below).
* **Own writes.** The reply's `changes` is applied directly when the replica's cursor equals the reply's
  `base` (nothing happened to the library between its last sync and the write, other than what the reply
  holds), the page is complete, and `dbId` matches; the cursor then moves to `revision`. Otherwise
  the client pulls, which finds the rest. Either way the replica equals one built by a full download
  (`toybox-remote/tests/sync.rs`).
* The app's window still reads a whole catalog from the replica (step 4 removes that).

### First download in bulk mode (build step 5)

A start from zero that needs more than one page (first connect, a replica from before the feed, `reset`, a forced
refresh) used to apply every page like an incremental one: ten indexes, eleven triggers and the search table kept up
with, 0.66 ms a row. Now (`Store::begin_bulk` / `finish_bulk`, `toybox-remote`'s `pull_pages`):

1. `begin_bulk` (one transaction): drops what only a finished replica needs (every secondary index except those the
   writing of a row looks things up in, the facet and search triggers, the `asset_search` table), empties the replica's
   rows (`user.db` is kept), and sets `meta.bulkLoad = 1` with no cursor. These statements are read from the schema text
   (`deferred_schema`), so there is one text of the schema.
2. Each page is an ordinary `Store::apply` with `Delta.bulk`: the same `row_in`/`write_row` as an incremental page
   (so the rows are the same), without the search rows and without pruning `changes`, on a connection with a large cache
   and the file mapped; the personal-data lookups are skipped when `user.db` has no rows at all. Pages are 5,000 rows
   (the server's maximum; 1,000 before), and the next page is requested while the current one is written. The cursor is
   written with each page as before, so an interrupted download goes on from it.
3. `finish_bulk` (one transaction): creates the indexes, fills `facets` and `asset_search` from the rows, recreates the
   triggers, clears `bulkLoad`.

While `bulkLoad` is set the replica is not listed by `Session::open` and the browse index does not describe it; a reply to
this client's own write is not applied on top of it (the pull goes on instead). A download that fits one page, and every
later sync, use the incremental path unchanged. `tests/sync.rs` checks that a bulk-built replica has the same rows, schema
objects, counts and search results as one built by a single page, that a favourite set before a forced refresh survives it,
that later edits, imports and deletes arrive on it exactly as on the other, and that half a download is resumed and ends whole.

## App queries (build step 4)

The window holds no catalog. It asks for a page of the visible list, for counts, and for single assets
by id; the answers come from `library.db` and `user.db` of every readable library through one read-only
connection (`infra::store::browse`, `ops::browse`, `src-tauri/src/commands/browse.rs`). The frontend side
is `services/browse.ts` (wrappers), `domain/pageCache.ts` (pages, stale replies), `state/assetList.ts`
(the list), `state/assetCache.ts` (items by id), `state/libraryCounts.ts` (counts).

### One connection over several libraries

`Session::open` makes an in-memory connection and `ATTACH`es each library's `library.db` as `l0, l1, ...`
and its `user.db` as `u0, u1, ...` with `file:...?mode=ro` (a library without a `user.db` gets an empty
in-memory stand-in with the same tables). Libraries are attached best rank first, so arm 0 never has to
look for a better copy. SQLite allows 10 attached databases by default, which is 5 libraries; the build
raises it to 40 (20 libraries) with `LIBSQLITE3_FLAGS=-DSQLITE_MAX_ATTACHED=40` under `[env]` in
`src-tauri/.cargo/config.toml`. Beyond the limit a plain sentence is the error ("Toybox can show at most N
libraries at once. Remove a library or choose fewer in the Libraries filter."). A build that did not pick the
flag up reports the limit it has the same way. Nothing here takes a write lock; each query is one read
transaction (a snapshot of each attached database).

### Commands

| Command | Reply |
|---|---|
| `open_library(library, force)` | `{ library, count, revision }`: migrations, catalog built if none is current, a remote library brought up to date; `force` rescans / downloads again |
| `scan_library(library, forceReseed)` | the same summary (the explicit rescan; no catalog) |
| `query_assets(filter, descending, offset, limit)` | `{ total, exact, rows: [{ library, asset }] }`: the page, assets with `user`, `thumb`, `contentHash`, variants inside. `exact: false` (a broad search only): `total` is what the pages seen so far prove (offset + rows, plus one when the page was full) and the exact number follows from `count_assets` |
| `count_assets(filter)` | the exact number of items the filter shows (a pass over the list for a broad search) |
| `query_ids(filter, descending, offset, cap)` | `{ ids: [{ library, id }], truncated }` (select all, the rows between two clicks) |
| `get_assets(items)` | `[{ library, asset }]` for item or variant ids, in the order asked |
| `facet_counts(kind, visibility, tagLimit, selectedTags)` | `{ total, libraries, categories, sources, publishers, licenses, formats, tags, tagTotal }` |
| `search_tags(kind, visibility, text, limit)` | `[{ value, count }]` |
| `type_counts(visibility)` | `[{ value: type, count }]`, each item once |
| `list_collections(kind, visibility)` | `[{ name, count }]` |
| `elsewhere(items, visibility)` | `[{ library, id, alsoIn: [{library,id}], otherVersions: [...] }]` |
| `rig_catalog(library)` / `list_rigs(library)` | the rigs, and the models and animations bound to a rig (for matching animations to models) |
| `pending_thumbs({ library, after, limit })` | `{ ids, next }` pictures still to make, a page of ids at a time |
| `set_favorite`, `set_collection`, `set_preview_choice`, `drop_personal_tags` | nothing: one transaction on `user.db`, the library revision does not move |
| `personal_tag_assets(library, limit)` | items that still carry personal tags (older versions) |
| `delete_assets` | `{ deleted: [ids], summary }`; `merge_seed_entries` returns the summary |

The filter is `{ kind, libraries, sources, publishers, licenses, formats, categories, tags, favoritesOnly,
collection, text, visibility, exclude, tagsAny, ranges, sort }`; `visibility` is the hidden-plugin rule as lists (`hiddenTypes, pluginsOff,
pluginsOn, retiredOn, retiredOff, ownerOff`, made by `pluginVisibility`). Removed from the window's path:
`load_catalog`, `save_catalog`, `list_pending_thumbs`.

* `kind: "*"` lists every type ("All assets"); `facet_counts`, `search_tags` and `list_collections` take it too
  (each type's numbers added up).
* `exclude` holds, per field (`sources, publishers, licenses, formats, categories, tags`), values that take an item
  out (`NOT IN`, and `NOT EXISTS` over library and personal tags). Exclusions never drive and never narrow the counts.
* `tagsAny`: with several tags, any one is enough (one `EXISTS … IN` instead of one per tag; the tag no longer drives).
* `ranges`: `[{ path, min, max }]`, a number of the stored record (`json_extract(asset, '$.<path>')`) within bounds;
  `path` must be words of letters, digits and `_` (`json_path`), anything else is refused. The paths come from the
  types' declared `properties` ([Asset types](asset-types.md#properties-sorting-and-range-filters)).
* `sort`: `{ key, descending }`, `key` one of `name` (default), `added` (`ord`), `changed` (`revision`), `publisher`,
  `source`, `license`, `format`, `category`, `type` or `prop:<path>`. A key other than the name is a column of each
  arm (`s`, with `z` = "has none", so items without it go last either way), then name, id, library; only the name
  order is served by the browse index, the others sort the matches of each page's query.

### The list

One `SELECT` per library arm, `UNION ALL`ed and merged by `ORDER BY name COLLATE NOCASE, id` (then the
library), `LIMIT ? OFFSET ?`. Each arm walks `assets_browse (type, name COLLATE NOCASE, id, content_hash)`
in order; `content_hash` is in the index so the copy probe needs no table lookup. Order is name, not the
catalog order the old window showed.

* **Filters.** `source, publisher, license, format, category` are columns (`format` is the format the window
  derives, `source/publisher/license` the recorded provenance); tags are `asset_tags` rows or `user.user_tags`
  rows; favourites and collections are `user.user_assets` / `user.user_collections`; hidden plugins are
  `type`/`plugin` lists.
* **Text.** A search of 3 characters or more whose matches are few (at most 2,000 per library) starts from
  `asset_search` (FTS5 trigram) plus personal tags that contain it; anything else (1-2 characters, or a search
  that matches a great many items) looks at the name while the list is walked, and at the tags through the
  tag vocabulary kept in `facets` (`IN` list of the tags that contain it) or, for a very broad needle, at each
  row's own few tags. **The total of such a search is not waited for**:
  `query_assets` answers `exact: false` when the exact count would need a pass over the list (a search that is not
  driven by a small set in every library), and the window asks `count_assets` once for the question, showing "N+ of M"
  until it arrives (`state/assetList.ts`). A search driven by a small set (at most 2,000 matches per library: the
  FTS5 trigram match, plus personal tags) and every filter the counts answer have their exact total in the first reply.
* **Driving sets.** A collection, favourites, one source/publisher/license/format, the first selected tag and a
  small search are copied (sizes estimated from `facets` / the personal indexes, the search by running it
  with a cap) into a temporary table the arm starts from (`CROSS JOIN` to `assets`); a page starts from it
  only when it is no bigger than about `6 * sqrt(rows)` (else walking the list for the first 100 matches is
  cheaper than sorting the set), a count whenever it is at most 20,000.
* **Duplicates.** A row is hidden when a better-ranked library in play holds an item of the same type with the
  same hash (`NOT EXISTS ... INDEXED BY assets_hash`), or, when either side has no hash yet, the same id. The
  second rule is a set (rows that share an id with an unhashed item of the better library, from the partial
  index `assets_unhashed`) built once per query and only when some item has no hash. With libraries chosen in
  the filter only those take part, so the grid shows their own copies de-duplicated among themselves; the
  counts always cover every library. This is the pair rule of `domain/duplicates.ts`; the greedy walk of the
  TypeScript can in contrived cases (a hashed copy hidden by an unhashed one that shares only its id) differ
  from the pair rule, which the tests port and compare on a mixed fixture.
* **With the browse index** (below) the list of a type that has no other constraint, in every library or in one, is a
  walk of one table in key order with copies left out by a column (`sh = 0`): no probe per row, no merge of arms.
  Filtered lists keep the per-library arms above.
* **Count.** A filter with at most one constraint (the type; one tag; one group of categories, sources, ...)
  is `facets` minus the hidden rows; any other filter counts its rows (`SELECT COUNT(*)` over the same arm,
  through the driving set when there is one).

### Counts

`facets (type, field, value, n)` is kept by triggers on `assets` and `asset_tags` (insert, delete, update of
the six columns), so every writer, whole-document or `Store::apply`, keeps it right; rows that reach zero are
deleted. A count is the sum over libraries minus the rows hidden by a switched-off plugin or as a duplicate.
The hidden rows are found in one pass over each library with a better-ranked one
(`INSERT INTO hid SELECT ... WHERE NOT (visible AND first copy)`, then `GROUP BY` against the hidden ids) and
remembered per revision / hash revision of the libraries involved, the type and the rules (`TALLIES`).
Personal tags that are not also library tags are added from `user_tags`. Tags in the rail are the most used
`tagLimit` plus the selected ones, with their total; tags that look like provenance are left out.

### The browse index (build step 5)

`<app-data>/browse-index.db` is derived data of this computer: the order of the merged list and the copy marks of all
its libraries. It is not in any `library.db`: a local library's database may be shared between computers through a
synced folder, and what is a copy of what depends on the libraries *this* computer has and how it ranks them. A remote
replica's `library.db` is local, but the same reasoning keeps the rank-dependent part out of it. The file has its own
`user_version` (1); a different one means "delete and rebuild". Nothing in it is the only copy of anything.

```sql
ix_libs  (id PK, pos, db_id, revision, rows)   -- the libraries in rank order and what the index has absorbed of each
merged   (type, name COLLATE NOCASE, id, o, sh, PRIMARY KEY (type, name, id, o)) WITHOUT ROWID   -- one row per item; o = library position
CREATE INDEX merged_id ON merged(o, id);
dup      (lib, id, by_lib, by_id, PRIMARY KEY (lib, id, by_lib, by_id)) WITHOUT ROWID   -- copies, both directions
shadowed (lib, id, type, category, source, publisher, license, format, tags JSON, PK (lib, id)) WITHOUT ROWID
hidfacets(type, field, value, n)                -- counts of the shadowed rows by field; kept by triggers on `shadowed`
```

`sh` is how many copies of the item sit in a library that ranks before its own; the unfiltered list of a type is
`SELECT id, o FROM merged WHERE type = ? AND sh = 0 ORDER BY name, id, o LIMIT ? OFFSET ?` (`o = ?` instead of `sh = 0`
for a session over one library, where nothing is a copy). The copy relation is the rule the queries already applied
(`domain/duplicates.ts`): same type, and both hashes present and equal, or the same id with either hash missing.

*Why this and not anchors or a materialised order per request.* `OFFSET` over the per-library arms cost about 1 us per
row skipped per library (probing the other libraries for copies, merging the arms). The index leaves one b-tree walk of
about 0.1 us per row, so a page at any depth is 20 to 80 ms at 1,000,000 items instead of 420 to 770 ms. It is
O(offset) with that small constant, not O(1): at 10 million items the deepest page would take about 0.8 s. A
materialised order per filter would have to be built per request (O(matches), about 1 s for the unfiltered 1,000,000) or
kept and renumbered on every insert; sparse anchors need a count structure that is maintained incrementally and
re-keyed on every insert before them. Both are more machinery than the walk needs at this size and were not built.

*Keeping it current* (`browse_index::ensure`, called when a session finds the index behind or absent):

* Each library's `changes` since the revision the index absorbed (`ix_libs.revision`) name the rows to redo. For each:
  its merged row, its copy pairs (and the counterparts' pairs) and its shadow row are removed; then, for rows that still
  exist, the merged row is inserted and the other libraries are probed for copies (`assets_hash`, the primary key), pairs
  are added, and `sh` is recomputed for every row a pair came or went for. A few index probes per changed row,
  however large the libraries. At most 1,000 changed rows are absorbed on the caller's thread.
* Anything else is a full rebuild, which runs on a background thread (one at a time per file) while sessions read the
  libraries directly with identical results: the first build, a different set or order of libraries (the user reordered
  libraries, added or removed one: ranks are positions in `ix_libs`), a replaced database (`dbId`), a revision that
  went backwards, a feed pruned beyond what the index absorbed (`changesFloor`), more than 1,000 changed rows, or a row
  count (`facets` sum) that disagrees with what the index counted: a write that left no trace in `changes` is caught that way.
  A rebuild is one transaction; readers see the old state until it commits. About 0.5 s at 100,000 items and 6.6 s at
  1,000,000 (two libraries).
* A session attaches the index read-only and uses it only if it lists the same libraries in the same order and every
  library it reads has the `dbId` and `revision` the index absorbed. A replica in the middle of its first download
  (`bulkLoad`, below) is not listed at all.

What the index serves: the unconstrained list (all libraries, or one), and the duplicate part of the counts. `hidden()`
reads `hidfacets` (plus the personal tags of shadowed rows, from `user.db`, which change without a library revision)
instead of one pass over every library with a better-ranked one: the first counts after a change cost a few
milliseconds of reads instead of 1.6 s at 1,000,000 items. **Not served:** a filter with any constraint (category, tag,
text, favourites, ...) keeps the arms and the `NOT EXISTS` probes; a Libraries filter that picks two or more but not all
libraries; and any session in which a plugin is switched off (`Visibility` hides something: the hidden items do not
shadow, which the index does not know). Those run the previous queries and the previous counts (the pass of 1.6 s at
1,000,000 items, remembered per revision).

Order changed in one corner: items of different libraries with the same name and the same id (different content, so
both are shown) sort by library rank, and **descending** now reverses that too (it used to keep rank order).

### Schema changes (still `user_version` 1)

`library.db`: `assets.plugin` (the importing plugin); `asset_tags.type`; table `facets` with
`facets_top (type, field, n DESC, value)` and ten triggers; indexes `assets_browse` (now with `content_hash`),
`assets_category (type, category, name COLLATE NOCASE, id)`, `assets_source|publisher|license|format
(type, x)`, `assets_unhashed (type, id) WHERE content_hash IS NULL`. `user.db`: `user_tags_tag (tag, asset_id)`,
`user_assets_favorite (asset_id) WHERE favorite = 1`. Both writers fill the new columns (`row_in` derives
`format` and the provenance columns); `adopt.rs` goes through them. Plans are asserted in
`store/browse_tests.rs` (`the_plans_use_indexes`), and the SQL is compared with ports of `applyFilter`,
`dedupeAssets` and the facet counts on a fixture of three libraries in the same file.

### Measured (`bench-library --final`, release; step 5)

`bench-library --final --dir D:\dev\toybox-bench\step5\final --sizes 100000,1000000 --reuse --reps 5 --sync-json 100000=a.json,1000000=b.json --table-out table.md`
runs, per size, the query benchmark (two libraries of N/2 rows, 10% of the second are copies by hash of the first, 2% share an id
with different content, 0.2% have no hash yet) and the write operations (one library of N rows, five repetitions on different
items, median), then prints this table. **The rows are written directly into the databases** (`infra::store::bench_data`:
`items`, derived rows, tags, search rows, counts; no files), because the import path takes about 13 minutes per 100,000 items
(about 20 hours for 1,000,000); the files of the items that are edited or exported are made by the benchmark. The sync rows come
from `toybox-remote/tests/bench_sync.rs` (a real server in process on a loopback port, `TOYBOX_BENCH_OUT=` writes its json), on a
library written the same way with `bench-library --direct --lib-name library --generate-only`.
Each read figure includes opening the connection, the page, the total when it is cheap, and loading the 100 assets. "Before" is the
table of step 4 (reads) and step 2 (writes, at 100,000); bold is a miss of the target (50 ms at 100,000, 150 ms at 1,000,000).

Reads (ms, median of runs; each includes opening the connection, the page, the total when cheap and loading the 100 assets)

| query | before 100000 | after 100000 | before 1000000 | after 1000000 |
|---|---|---|---|---|
| open / start (summaries + type counts) | - | 11.6 | - | 24.6 |
| first page (100 rows + total) | 20 | 16.8 | 58 | 17.6 |
| middle page | 45 | 20.7 | 423 | 48.8 |
| last page | 71 | 24.4 | 772 | 75.8 |
| text, 2 characters (page; total follows) | 47 | 19.5 | 411 | 26.3 |
| text, rare (page + total) | - | 15.0 | - | 21.3 |
| text, 5 characters, 1% of items (page) | 69 | 29.0 | 582 | 72.3 |
| text, 5 characters, broad (page) | 75 | 23.6 | 728 | 27.8 |
| tag, common | 31 | 30.4 | 63 | 22.6 |
| tag, rare | 19 | 17.0 | 70 | 30.2 |
| category | 20 | 17.8 | 59 | 17.8 |
| favourites | 24 | 23.7 | 72 | 30.4 |
| collection | 23 | 22.1 | 66 | 21.4 |
| category + tag + text | 57 | **62.0** | 326 | **337.9** |
| facet counts, warm | 12 | 11.6 | 52 | 13.1 |
| facet counts, first after a change | 132 | 49.3 | 1581 | 50.6 |
| facet counts, first with a plugin hidden | 132 | **148.5** | 1581 | **1907.6** |
| type counts | 8 | 4.6 | 47 | 4.5 |
| get 100 assets by id | 8 | 8.5 | 9 | 9.2 |
| select all ids (10,000) | 16 | 6.4 | 53 | 6.4 |

Writes (seconds, 100 items per operation unless said)

| operation | before 100000 | after 100000 | after 1000000 |
|---|---|---|---|
| import 100 | 0.740 | 0.961 | 0.789 |
| edit tags of 100 | 0.280 | 0.313 | 0.307 |
| delete 100 | 0.450 | 0.417 | 0.404 |
| ingest a 100-item package | 1.180 | 1.351 | 1.107 |
| set a favourite | - | 0.005 | 0.004 |
| set one thumbnail state | 0.007 | 0.005 | 0.005 |

Sync (loopback server, library written directly)

| | after 100000 | after 1000000 |
|---|---|---|
| server: first page of the feed from 0 (s) | 0.132 | 0.131 |
| server: a page 80% into the feed (s) | 0.148 | 0.129 |
| changes feed for 100 rows (s) | 0.018 | 0.021 |
| client takes those 100 rows (s) | 0.051 | 0.052 |
| first sync (s) | 10.1 | 144.7 |
| first sync (rows/s) | 9902 | 6912 |
| first sync before step 5 (rows/s; 66 s for 100,000 rows) | 1500 | about 1500 |

What the table says, and what it does not:

* **Deep pages**: the middle and the last page cost 49 and 76 ms at 1,000,000 items (423 and 772 before): the walk of one table
  by the browse index, about 0.1 us per row skipped. Under the target, but O(offset), not constant (see "The browse index").
  Cause of the old cost found on the way: `Session::open` asked whether a library has any item without a hash with a query the
  planner answered by scanning the table (about 45 ms per library at 1,000,000 rows); it is now forced through the partial
  index `assets_unhashed` (0.1 ms). That was the floor under every query at that size.
* **Text**: the 2-character and the broad 5-character search answer their first page in 26 to 28 ms at 1,000,000 because the page
  no longer waits for the total (the old cost was almost all the total: a pass over the whole list, 0.7 to 1.5 s). The exact
  total follows from `count_assets` (about 0.7 s at 1,000,000 for a broad needle, off the page's path). "Rare 5 characters" in the
  step 4 table was `tag29`, which in this data is on 1% of the items (about 54,000 at 1,000,000, past the 2,000 a search may
  have to start from the FTS5 index), so it was not rare: its cost was the walk plus the total. A really rare needle
  (`tower 976`, about 25 rows at 1,000,000) starts from the FTS5 match and costs the floor of a query (21 ms) at either size,
  total included. The 1% needle's page is 72 ms at 1,000,000: the walk reads about 10,000 rows to find 100 matches.
* **Misses of the targets, with their causes.**
  * *category + tag + text* (62 ms at 100,000 / 338 ms at 1,000,000): three constraints, none selective enough to drive
    (category 4% of the items, tag 20%, text 5% in each library) and only 189 matches at 1,000,000: the page walks the category
    list and tests tag and text on every row, about 3.5 us per row. An intersection of the three sets would be the fix; not built.
  * *first counts with a plugin switched off* (148 ms / 1.9 s): the hidden items do not shadow a copy, which the browse index
    does not know, so the pass over every library with a better-ranked one runs (it is remembered per revision). The common case,
    no plugin switched off, is the row above it (49 and 51 ms: one write of a row, the catch-up of the index, and the counts).
  * *facet counts, first after a change* is 49 ms at 100,000 against a target of 50 ms: it includes the benchmark's own write.
* **Writes** do not depend on the size of the library (1,000,000 is within 10% of 100,000 or faster) and none regressed badly
  against step 2: import 0.96 s against 0.74 s, edit 0.31 against 0.28, ingest 1.35 against 1.18, delete 0.42 against 0.45. The
  differences of up to 30% at 100,000 are not explained by a cause I found (the figures are medians of five runs on a machine that
  was also doing other work; a first run on a fresh library, made before the generator emptied its write-ahead log (as big as the library), was 4 to 8 times
  slower than a second run on the same library, which is why the generator now empties it). No index or trigger was
  dropped: the writes are dominated by file work and by the pages of the rows' b-trees, not by one index.
* **First sync**: 10.1 s for 100,000 rows (9,900 rows/s) against 66 s before, 6.5 times faster; 145 s for 1,000,000 (6,900 rows/s),
  4.6 times faster than the same 0.66 ms/row (there is no old figure at 1,000,000; the old path was not run there). The target of 5
  times is met at 100,000 and missed at 1,000,000: 47 s of it is `finish_bulk` (about twenty index builds, the counts and the search rows;
  3.9 s at 100,000, so it grows faster than linearly), about 98 s are the pages (a page is written in 0.3 to 0.5 s, flat with depth
  once the connection and its cache stay open and the write-ahead log is not emptied at every commit; the next page is fetched
  meanwhile; the server's side of a page is 0.13 s at any depth).
* Not measured: a real library (these rows are synthetic, with small files absent), a replica with variants or personal data in
  `user.db` (the bulk path then reads `user.db` as an incremental page does), the app itself.

### What is still O(N) (end of step 5)

* A page of the unfiltered list at depth `d`: `d` rows of one b-tree walk, 0.1 us each (76 ms at the end of 1,000,000 items).
* The exact total of a broad search (0.7 s at 1,000,000, asked for apart from the page), and a broad search's page: the rows read to
  find 100 matches, about 100 / density.
* A filter of several constraints none of which is selective (category + tag + text: 0.34 s at 1,000,000).
* The first counts after a change in a session where a plugin is switched off (1.9 s at 1,000,000), and the lists and counts of a
  Libraries filter that picks some but not all of three or more libraries (the per-library arms and probes).
* The first build of the browse index and its rebuild after a change of libraries or an unexplained difference (6.6 s at 1,000,000,
  on a background thread, queries meanwhile read the libraries directly), and the catch-up of more than 1,000 changed rows.
* A first download or reset of a replica: 145 s at 1,000,000 rows (47 s of it building the indexes at the end).
* `pending_thumbs`: a page of ids at a time, two `stat`s for each id that has a picture (1.9 s over 1,000,000 ids in all).
* `remote.json` still holds the two thumbnail maps and is rewritten whole when pictures change.
* A selection dialog for transfer or tagging loads every selected item (`get_assets`, now asked for in pieces of 2,000); delete
  lists the first items only. Selecting 100,000 items and transferring them loads 100,000 assets.
* Not O(N) any more: the items-per-type counts after a change, the pictures listing of a first sync (a listing of the picture
  folder and a lookup of the names found, was one `stat` per item), the unhashed-items probe of opening a session.

Tracing: `TOYBOX_TRACE=1` (commits), `TOYBOX_TRACE_OPEN=1` (what opening a browse session spends), `TOYBOX_TRACE_SYNC=1` (a first
download's pages, and the end of a bulk download) print on stderr.

## Build order

1. Store in `toybox-core` with a compatibility layer: the existing manifest and
   cache functions read and write through SQLite, so every current code path and
   test keeps working. Migration step 4.
2. Hot paths made incremental: import and ingest, metadata edits, delete,
   thumbnail state, content hashes, by-id reads (done: see "Incremental operations").
3. Server: revision counter, `/changes`, reply shapes, run-to-completion writes.
   Client: incremental sync, parallel uploads. (done: see "Server protocol")
4. App: paged queries and facet counts from the database; the whole-catalog
   commands are removed. (done: see "App queries")
5. Benchmark with a generated library of 1,000,000 items. (done: see the end of "App queries"; `bench-library --final`)
6. Scale: the browse index, the total apart from the page, scaled scrolling in the grid, the bulk first download. (done: see "The browse index", "First download in bulk mode")

## Incremental operations (build step 2)

| Operation | Reads and writes |
|---|---|
| `ops::import::merge_entries` | validates and derives the incoming entries; absorbs by `pkey`, replaces by id; places their files; one `Store::apply` |
| `ops::transfer::ingest_package` | owners of the package's ids in one query; chunks of 20 go through `merge_entries`; personal data written for the landed items only |
| `ops::transfer::export_package` | the asked entries, their personal rows, the clips cut from them (`clip_source`) |
| `ops::metadata::update_entries` | the edited entries and their asset rows; the 5000-edit cap and all-or-nothing stay |
| `ops::delete::delete_from_library` | `asset_files` / `thumb_name` queries; removes the rows of the deleted items; rigs without users go |
| `ops::thumbs::set_thumb_state` | one `user.db` row |
| `ops::thumbs::list_pending_thumbs` | streams asset ids against one listing of the thumbnail folder |
| `Store::load_assets` / `load_top_assets` / `load_item_groups` | the rows of the asked ids (variants too), their personal rows, nothing else |
| `ops::content_hash::hash_new` / `fill_assets` / `rehash` / backfill saves | point lookups and writes of the keys concerned |

`TOYBOX_TRACE=1` prints where a commit spends its time (import phases, rows,
retiring the originals) on stderr. `cargo run --release --bin bench-library --
--dir D:\dev\toybox-bench\run --items 100000` builds a synthetic library
through the real import path and times the operations above (see its header). `--generate-only --lib-name library` leaves a library that
`toybox-remote/tests/bench_feed.rs` serves for the wire measurements below.


### What still depends on the size of the library

Measured with `bench-library` (release, a synthetic library built through the real
import path; 100 items per operation; seconds):

| op | 2k | 20k | 100k |
|---|---|---|---|
| import 100 | 0.66 | 0.76 | 0.74 |
| edit tags of 100 | 0.09 | 0.22 | 0.28 |
| delete 100 | 0.19 | 0.30 | 0.45 |
| ingest a 100-item package | 0.89 | 0.96 | 1.18 |
| set one thumbnail state | 0.005 | 0.006 | 0.007 |
| export a package of 100 | 0.04 | 0.04 | 0.04 |
| hash 100 new items | 0.03 | 0.04 | 0.04 |
| list pending thumbnails (streams N ids) | 0.006 | 0.03 | 0.14 |
| load the whole catalog (no longer used by the window; `scan_library`, the version 1 `GET /catalog`) | 0.09 | 0.80 | 4.1 |

Import and ingest are file work: placing an item copies and verifies its files
(each is hashed before, during and after the copy) and retires the staged
originals; the rows are about 25 ms of the 740 ms per 100. Edits and deletes grow
from 2k to 20k and then flatten, because the rows of 100 random items sit on
distinct pages of ten indexes and the commit writes (and checkpoints) those pages;
at 2k the same pages are shared. Still O(N) by nature: the whole-catalog reads
of an explicit rescan (`load_catalog_inner`, `scan_library_catalog`), the once-per-library move of personal
tags of a library from before library tags, the window's pending thumbnails (now streamed in pages of ids, each page
costs what it scans), `compatible_previews` (it matches
every rigged asset of the other kind), the first write to a library that has no
current catalog (a rescan), and the once-per-process tidy-up of empty set folders.
The wire no longer is (step 3, below), with the exceptions listed there.

## The wire (build step 3)

One metadata edit of 100 items, end to end through the real server and client (loopback HTTP,
release build, generated libraries, `toybox-remote/tests/bench_feed.rs`; seconds; the three
rounds of a run, median in the table):

| | 20k | 100k |
|---|---|---|
| (a) server handles `POST /assets/metadata` of 100 items (edit, reply with its 100 rows) | 0.20 | 0.32 |
| (b) `GET /changes` for those 100 rows | 0.007 | 0.007 |
| (c) a following client takes them (request, row-level apply into its replica) | 0.15 | 0.25 |
| the client's own edit: server work plus applying the reply directly | 0.34 | 0.53 |
| first connect: full paged download (1000 rows per page; once per client, O(N) by nature) | 7.7 | 66 |

The feed does not depend on the library at all. (a) and (c) rise 1.5x for 5x the items, the
same effect as "edit tags of 100" in the table above: the rows of 100 items scattered over a
big library sit on distinct pages of ten indexes, so the commit writes more pages (the replica
is written like the library, with the same indexes). Neither grows in proportion to N.

Before step 3, at 20k (reconstructed from the pieces version 1 ran for every change, because the
old server and client are not kept: one `GET /catalog` built after an edit = a whole-catalog read,
strip, serialise; the client's parse + `merge_preserving_user` + diff-save of the whole catalog):
the server spent about 0.2 s on the edit and then **0.97 s and a 17 MB body** on the catalog
(1.1 s and 17 MB in an earlier run), and a client **0.97 s** (1.0 s) taking it, per change, on
any change, however small, by anyone. Now a following client needs (c), 0.15 s at 20k, and the
17 MB document is not built unless a version 1 client asks for it.

### What is still O(N) on the wire

* `GET /catalog` for version 1 clients: built on demand at the first request after any change
  (read of every row, strip, serialise), then served from memory for that revision. Not built by
  anything a version 2 client does.
* `GET /thumbs` without `since`: lists every picture (a client does it once, and again on a forced
  refresh). `GET /thumbs?since=` is proportional to the pictures saved since.
* The server's first fingerprint pass after each start drops hashes of long-gone folders
  (`prune_hashes`: one scan of the hashes against the asset rows); the passes after that read the
  pending index only. Hashing itself is file reading, in the background.
* `ensure_ready` reads four counters on each request, and does the migration/rescan work only when the
  library database was replaced or changed from outside (a library that has no current catalog
  rescans once, as for any write).
* A client's first sync, `reset`, a forced refresh and a replica made before the feed: a full
  download, which is O(N) and paged. Since step 5 it runs in bulk mode (see "First download in bulk mode"): pages of
  5,000 rows applied without the secondary indexes, then one pass to build them. The remaining costs, at 100,000 rows
  (14 to 16 s, was 66 s): writing the rows (about 6.5 s), building the indexes, counts and search rows at the end
  (3.7 s), fetching and decoding pages (2.6 s, partly overlapped with writing), and the first `GET /thumbs`, which
  looks for a picture of every item of the library on the server with one `stat` each (1.6 s here, where there are
  no pictures; O(N) and not changed). Serving a page of the feed from 0 costs the same 0.13 s per 5,000 rows at any
  depth of the feed (`since=0` plus a cursor is a range scan of `changes_revision`, then point reads of the rows).
* The window no longer reads the catalog back from the replica: `RemoteLibrary::refresh`, `delete` and `import_rows` return
  nothing but their outcome (`refresh_catalog`, `delete_assets` and `import` remain, built on them, for the tests).
* `remote.json` still holds the two thumbnail maps (`sharedThumbs`, `localThumbs`: every
  picture this client knows of) and is rewritten whole when pictures change; with no pictures it
  is small, with many it is O(pictures). Moving them into the replica database is left for step 4.
* Row order: the replica's catalog order is the order rows arrived (revision, then id), not the
  server's. Nothing but the window's sort order depends on it.
* The `changes` table has a row per asset ever changed (live rows) and per deletion in the last
  20,000 revisions.
