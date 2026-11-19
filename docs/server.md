# Toybox server

`toybox-server` serves **one Toybox library** over HTTP so that people can
browse, download and (with the right token) change it from the Toybox desktop
app. It is a small headless program (crate `src-tauri/crates/toybox-server`)
built on the same `toybox-core` library logic as the app, so the library on the
server is the ordinary [on-disk format](library-storage.md): a library can be
seeded by copying a folder up, and copied back down.

What the server holds is the library, including its **tags** (descriptive
keywords stored in the item's entry in `library.db`, see
[Libraries](libraries.md#tags)). What stays on each client:
favourites, collections and preview choices (see
[Libraries](libraries.md#per-library-app-data)), and thumbnails the client
renders itself. The server has no user metadata and its catalog never contains
any.

The step-by-step setup for an Ubuntu machine is in
[deploy/README.md](../deploy/README.md). This page is the reference.

## Setup in a few commands

```sh
cd deploy
sudo bash install.sh --source .. --domain assets.example.com
```

That builds the program, creates `/var/lib/toybox`, starts the hardened systemd
service on `127.0.0.1:8787`, and puts Caddy in front for automatic HTTPS. It
prints the address and an admin token **once**. In Toybox, add a remote library
with that address and token.

Without the installer:

```sh
toybox-server init --data /var/lib/toybox --name "My Toybox"   # prints the admin token once
toybox-server serve --data /var/lib/toybox                      # 127.0.0.1:8787
```

## The data folder

```text
/var/lib/toybox/
  server.json   name and tokens (SHA-256 hashes only), written atomically
  library/      the Toybox library: _library_config/ and library/ inside;
                _library_config/library.db holds the items, derived assets, tags,
                content fingerprints, rigs and change log (the truth)
  cache/        shared thumbnails (and the place of user.db, which holds personal
                data on a client and nothing on a server); rebuildable
  uploads/      imports in progress; temporary
```

Before the databases (migration step 4) `library/_library_config/catalog.json` was the
manifest, and `cache/` also held `catalog.json` and `content-hashes.json`. The first
start of a current server converts them into `library.db` and moves the old files
into `library/_library_config/migration-backups/0004-<stamp>/`; see
[SQLite library storage](sqlite-storage.md). The library database is not meant to be
edited by hand: change the library through the API or the desktop app. Opening it
read-only with a SQLite tool is fine.

`--data DIR` (or `TOYBOX_DATA`) names it; the default is `/var/lib/toybox`.

## Command line

```text
toybox-server init   [--name TEXT]                   set up the folder, print an admin token once
toybox-server serve  [--listen ADDR:PORT]            run (default 127.0.0.1:8787, or TOYBOX_LISTEN)
                     [--max-file-mb N] [--max-upload-mb N]
toybox-server token add --name TEXT --role read|write|admin
toybox-server token list
toybox-server token revoke NAME-OR-ID
toybox-server status [--listen ADDR:PORT]            setup summary; is a server running?
```

Every command takes `--data DIR`. Exit status: `0` success, `1` failure (the
message says what and what to do), `2` wrong usage.

- `init` refuses to run twice. If `library/` already holds a Toybox library
  (copied up before), it is adopted and indexed; if it holds anything else, it
  refuses rather than scaffold into it.
- `token add` prints the secret once. A **running server picks up changes made
  by the command line without a restart**: it reads `server.json` again when
  the file changes (and at least every two seconds). If the file becomes
  unreadable, the server refuses all sign-ins (`503 settings_unreadable`) until it
  is fixed, rather than keep honouring tokens that may have been revoked.
- `token revoke` and the API both refuse to remove the last admin token.
- Run it as the service user (`sudo -u toybox toybox-server ...`) so the files
  keep their owner. (The program also keeps the owner of `server.json` when it
  rewrites it, as a safety net.)
- `serve` with an address other than loopback prints a warning: tokens travel
  in every request, so it must sit behind HTTPS.

## Roles

Ordered `read < write < admin`; each includes the one before.

| Role | May |
| --- | --- |
| `read` | browse the catalog, download files, download shared thumbnails, describe items as a transfer package (`/assets/export`) |
| `write` | also upload shared thumbnails, import (uploads + commit), copy items in from another library (uploads + ingest), delete items, edit library tags and provenance (`/assets/metadata`) |
| `admin` | also list, create and revoke tokens |

Tokens look like `tbx_` plus 43 URL-safe characters (32 random bytes). They are
sent as `Authorization: Bearer <token>`. Only the SHA-256 is stored; hashes are
compared in constant time; tokens are never logged.

## API reference (version 1)

Base path `/api/v1`. All bodies are JSON (`Content-Type: application/json`)
unless a route says otherwise; field names are camelCase. Every route except
`GET /health` needs `Authorization: Bearer <token>`.

### Errors

Every error has the same shape and a plain-language `message` meant to be shown
to a person:

```json
{ "error": { "code": "forbidden", "message": "This token cannot do that. It needs the write role." } }
```

| Status | Meaning (`code`) |
| --- | --- |
| 400 | the request is malformed (`bad_request`, `bad_path`, `bad_upload_path`, `not_png`, `no_ids`, `no_entries`, `no_edits`, `too_many_ids`, `invalid_name`, `invalid_metadata`) |
| 401 | missing or wrong token (`unauthorized`); carries `WWW-Authenticate: Bearer` |
| 403 | the token's role is too low (`forbidden`) |
| 404 | no such item, file, upload or token (`not_found`) |
| 409 | conflicts (`last_admin`, `name_taken`, `upload_busy`, `path_conflict`) |
| 413 | a size limit was hit (`too_large`) |
| 416 | unsatisfiable `Range` (`range_not_satisfiable`) |
| 422 | the library rejected the data (`import_failed`, `delete_failed`, `metadata_failed`) |
| 429 | locked out after wrong tokens (`locked_out`, with `Retry-After`) or too many imports in progress (`too_many_uploads`) |
| 503 | `settings_unreadable`: `server.json` cannot be read |
| 500 | `internal`, `library_error` |

Server folder names are removed from messages.

### Sign-in protection

A wrong token is answered after a short pause (400 ms) with 401. After 10 wrong
tokens within 10 minutes from one address, that address is refused for 15
minutes (`429 locked_out`, `Retry-After` in seconds) even with a valid token.
The address is the connection's peer; when the peer is loopback (the reverse
proxy on the same machine), the last entry of `X-Forwarded-For` is used instead.
From any other peer that header is ignored. The counters are in memory only.

### `GET /api/v1/health`

No token. `200 {"status":"ok"}`.

### API versions

`apiVersion` is **2**. Version 2 added the revision feed (`GET /changes`) and write replies that
carry the rows they changed instead of a catalog. A request that sends `X-Toybox-Api: 2` is answered
in version 2 shapes; one without it (an app from before the feed) gets the version 1 shapes described
as "version 1" below. The full description of the feed, its retention rule and the client
algorithm is in [SQLite library storage](sqlite-storage.md#server-protocol-api-version-2).

### `GET /api/v1/info` (read)

```json
{ "name": "My Toybox", "serverVersion": "0.4.1", "apiVersion": 2, "role": "read",
  "revision": 134, "dbId": "7c1e…", "thumbRevision": 7 }
```

`role` is the role of the token used. `revision` is the library revision, an integer that goes up
with every committed change to the library: imports, deletes, edits and **content fingerprints**
arriving (below). `dbId` identifies the library database. Without the version 2 header `revision` is
the string version 1 clients know (it is also the catalog's ETag).

### `GET /api/v1/changes?since=<revision>&limit=<n>` (read)

What changed after revision `since`, oldest first, in pages: `{ dbId, reset, schemaVersion, since, next,
after, revision, thumbRevision, more, assets, rigs, deleted }`. `assets` are the changed assets exactly as
`GET /catalog` serves them, `rigs` the rigs they bind to, `deleted` ids that left. `since=0` is a full download.
Follow `next` (and `after` when `more` is true). `reset: true` means start again from 0 (the library was
replaced, or `since` is older than the deletions the server remembers). `dbId=<id>` names the database the
cursor belongs to. Read from the revision index; no catalog is built.

### `GET /api/v1/catalog` (read)

The library catalog, built by `toybox-core` exactly as the desktop app builds
one (same JSON as `load_catalog`: `{ rigs, schemaVersion, libraryRoot, assets }`),
with these differences:

- No personal metadata. For every item and variant: `user.favorite` is false,
  `user.collections` is empty, no `user.previewAssetId`, and `user.tags` holds
  only tags derived from import provenance (`source: …`, `publisher: …`,
  `license: …`). `user.provenance` is kept, **without** `filesystemPath` (the
  path on the importer's machine).
- **Tags are kept.** An item and each variant carries `tags: ["footstep", …]` at
  its top level (not inside `user`) when the entry has any: normalised,
  lower-case, never with a colon. They are the library's, so every client sees them (and a client that has the same item in
  another library writes a tag change to each copy);
  `POST /assets/metadata` below is how they are edited. The field is absent when empty.
- `libraryRoot` is `""`. All paths in the catalog are library-relative; fetch
  them with `GET /files/{path}`.
- `thumb` is always `{ "state": "missing" }`. Whether a shared picture exists
  is answered by `/thumbs`.
- Every item and variant carries `contentHash` once the server has computed it: a
  lower-case hex SHA-256 over the item's files, the same for byte-identical content
  whatever the id, name or pack (definition in
  [Libraries](libraries.md#duplicates-across-libraries)). A client uses it to
  recognise a copy in another of its libraries; where it is absent (older server, not
  computed yet, files unreadable) clients fall back to the id.

Headers: `ETag: "<revision>"`, `Cache-Control: private, no-cache`. Send
`If-None-Match` to get `304 Not Modified` with no body. This is the **version 1** way to read the
library: the catalog is built when asked for and kept for its revision, and no write builds it. A big
library should be followed through `/changes` instead.

**Fingerprints.** They are kept in the `hashes` table of `library.db` (before
migration step 4: `cache/content-hashes.json`, outside the library), keyed by the item's
content-addressed storage folder, and are never recomputed for a folder that is in
it. Items added through the API are fingerprinted as part of the commit or ingest, so
the response and the next catalog already carry them. At start, and then once a
minute, the server looks for items that have none (a library copied up, or edited on
disk) and fingerprints them in the background: two files at a time, a thousand items
per step, off the write lock, saving the hashes as it goes. Each step is a library revision,
so **`revision` and the ETag change** and the rows that show the new fingerprints are in
`/changes`; version 1 clients that send `If-None-Match` fetch the catalog again. I
chose to make the fingerprints part of `revision` rather than leave `revision` for the
library content alone, because the ETag has to be the revision (the delete and ingest
answers use it as one) and a catalog with new fields is a new catalog. A pass costs
reading every file of the library once; items with missing or unreadable files are
skipped and not retried until the next start.

### `GET /api/v1/files/{path}` (read)

A file of the library; `path` is library-relative, for example
`library/audio/3f9c0a41de7b2c10/footsteps.wav` (percent-encode as usual).

- **Only `library/**` is served.** `_library_config/` (`library.db` and
  migration backups), the caches, `server.json` and files of imports not yet
  committed (`library/_import_*`) are never served, nor is anything reached
  through a symbolic link that leaves `library/`. The rest is everything the
  catalog refers to, including original source files kept with items.
- The path is checked textually (no `..`, `.`, empty parts, absolute paths,
  backslashes, `:`, control characters; max 1024 characters) and then
  canonicalised against the real library root. Refusals are `400 bad_path` or
  `404 not_found`.
- Streams from disk. `Range: bytes=a-b`, `a-` and `-n` are supported (`206`,
  `Content-Range`); a range outside the file is `416` with
  `Content-Range: bytes */<size>`; several ranges at once get the whole file.
  `Accept-Ranges: bytes` is always sent. `If-Range` with the file's ETag is
  honoured.
- `ETag` (strong, size + modification time) with `If-None-Match` → `304`.
- Paths whose folder or file name is a content hash (`library/<type>/<16 or 64
  hex>/…`, `library/models/_resources/<hash>.<ext>`) are immutable:
  `Cache-Control: private, max-age=31536000, immutable`. Others get
  `private, no-cache`.
- `Content-Type` is chosen from the extension; `X-Content-Type-Options: nosniff`.

### Shared thumbnails

The desktop app renders thumbnails itself; the server can hold a shared copy so
that a read-only friend does not have to render thousands. Ids are the catalog's
raw asset ids (they contain `://`, so percent-encode: `uid%3A%2F%2Fabc`).
Items and variants are accepted.

- `GET /api/v1/thumbs` (read) →
  `{ "revision": "…", "thumbRevision": 7, "thumbs": [ { "id": "uid://abc", "version": "18d6f…-2a1c" } ] }`
  for every item that has a picture. `version` changes when the picture does.
  `ETag` + `If-None-Match` → `304`. `GET /api/v1/thumbs?since=<thumbRevision>` lists only the
  pictures saved (by `PUT`) after that point in the thumbnail log, so a client need not list them all.
- `GET /api/v1/thumbs/{id}` (read) → `image/png`, `ETag`, `If-None-Match` → `304`;
  `404` if the id is unknown or has no picture yet.
- `PUT /api/v1/thumbs/{id}` (write), body is the PNG (must start with the PNG
  signature, at most 2 MB) → `204`. `404` for an id that is not in the library.
  Uploading a thumbnail does not change the library `revision`; it moves `thumbRevision`.

### Importing: uploads and commit (write)

Mirrors the desktop importer: stage files, then commit seed entries. The seed
entries are the same JSON objects `merge_seed_entries` takes in the app. The
files they refer to are staged under the private namespace
`library/_import_<name>/…`, exactly as importers do (the entries' `pack` is that
`_import_<name>`).

1. `POST /api/v1/uploads` → `201`
   `{ "id": "<32 hex>", "maxFileBytes": …, "maxUploadBytes": …, "maxFiles": …, "idleTimeoutSeconds": 3600 }`.
2. `PUT /api/v1/uploads/{id}/files/{path}`, body is the raw file (any content
   type), streamed to disk → `200 {"path": "…", "size": n}`. `path` must look
   like `library/_import_<name>/<folder>/<file>` (at least four parts; `<name>`
   letters, digits, `_`, `-`). Anything else is `400 bad_upload_path`; `..`,
   absolute and backslash paths are `400`. Putting the same path again replaces
   the file. A file and a folder with the same name is `409 path_conflict`.
   A name that Windows cannot store is `400 bad_path` even though Linux would
   accept it: the characters `< > " | ? *` and the device names `CON`, `PRN`,
   `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9` (with any extension). Two files of
   one import whose paths differ only in upper and lower case are `409
   path_conflict`, because they would be one file on Windows. (A library is
   copied between the two systems.)
   A file larger than the per-file limit, or a total over the per-import limit,
   or more than 5000 files is `413 too_large`; a refused file leaves nothing
   behind.
3. `POST /api/v1/uploads/{id}/commit` with `{ "entries": [ … ] }` (at most 5000
   entries, body up to 32 MB):

   ```json
   { "revision": 135, "base": 134, "imported": ["uid://sky1"],
     "errors": [ { "index": 1, "id": "uid://x", "message": "…" } ],
     "uploadKept": true, "changes": { "…": "the /changes page for since=base: the rows this commit wrote" } }
   ```

   (`revision` is a string and `base` and `changes` are absent for a version 1 client.)

   Entries are checked one by one (server rules first: `pack` must be a
   staging name, no `storagePath`, no path tricks in `name`, `category`,
   `file`, `files`, maps and origin files; then core's own parsing). The valid
   ones are merged in one go with `toybox-core`'s `merge_entries`, which
   validates everything before it writes anything; if that fails, the entries
   are retried one at a time so one bad entry does not sink the rest. `errors`
   lists the entries that did not make it (`index` is the position in the
   request). Status `200` when at least one item was imported; `422
   import_failed` (same body fields next to `error`) when none was. When
   anything failed the upload is **kept** (`uploadKept: true`) so the client can
   upload the missing files and commit again; when everything succeeded it is
   removed. Results are not partial inside one item: an item (with its variants)
   is imported whole or not at all.
4. `DELETE /api/v1/uploads/{id}` → `204`: cancel and remove the staged files.
   `409 upload_busy` while files are arriving or a commit runs.

Uploads are kept in `uploads/` and removed after one hour without activity;
they do not survive a server restart. At most 16 uploads are in progress at a
time (`429 too_many_uploads`).

How the files get to core: at commit the staged files of the packs named by the
entries are hard-linked (copied if the disk differs) into
`<library>/library/_import_<name>/…`, where `merge_entries` expects staged
files; core then publishes them into content-addressed managed storage and
removes the `_import_` folder; the server removes anything left, and the upload
folder is untouched until the commit has succeeded.

### Sources the server already stores

A source file an importer keeps with an item (a material's `.sbsar`) is stored once per library
(`library/<type>/_sources/<sha256>/<name>`), so a client need not send one that is there. Two
write-role routes let it skip them; a client that does not know them sends every file and the
server stores each source once all the same:

- **`POST /api/v1/sources/have`**, body `{ "sha256": [ … ] }` (at most 20000): answers
  `{ "have": [ the digests it stores ] }`.
- **`POST /api/v1/uploads/{id}/links`**, body `{ "files": [ { "path", "route", "sha256" | "from" } ] }`:
  puts a file into the upload without receiving it. `route` is `files` (a staged import path,
  `library/_import_…`) or `package` (a package path); `sha256` makes it from the stored source with
  that content (409 `source_missing` if it is not there any more), `from` from another file of the same
  upload (the same route) that was uploaded. Send the links after the uploads, before `commit` or `ingest`.

The server converts a library made before this to the layout when it starts (migration step 5,
see [Managed library storage](library-storage.md)); the first start after an update can take as long
as hashing the kept sources takes.

### Copying items from another library: export, package upload, ingest

This is how the Toybox app copies or moves items between a server and any other
library (local or another server) with the same code a copy between two local
libraries uses (`toybox_core::ops::transfer`), so ids, conflicts, packs, categories,
variants, kept originals and thumbnails behave identically. See
[Libraries](libraries.md#the-two-halves-and-the-package).

**`POST /api/v1/assets/export` (read)**, body `{ "ids": ["uid://sky1"], "includeClips": false }`
(raw ids of top-level items, at most 10000) → a transfer package:

```json
{ "version": 1, "sourceLibrary": "",
  "assets": [ { "id": "uid://sky1", "name": "Noon", "kind": "skybox",
                "entry": { "…": "the entry as the library records it (items.entry)" },
                "files": [ { "path": "library/skybox/3f9c0a41de7b2c10/px.png", "size": 1234, "sha256": "…" } ],
                "user": {}, "thumbs": [ { "id": "uid://sky1", "file": { "path": "thumbs/uid___sky1.png", "size": 90, "sha256": "…" } } ] } ],
  "failed": [ { "id": "uid://gone", "name": "uid://gone", "kind": "model", "reason": "No longer in the source library" } ] }
```

The package holds no bytes: fetch each `library/...` path with `GET /files/<path>` and each
`thumbs/...` path with `GET /thumbs/<id>` (the `id` is in `thumbs`). `user` is always
empty (the server has no personal metadata). Items that cannot be described (unknown
id, missing file, a glTF with several buffers) are listed in `failed` with a reason and
do not affect the others. `includeClips` also describes the animation clips extracted
from the selected models.

**`PUT /api/v1/uploads/{id}/package/{path}` (write)** stores one file of a package the
client is about to ingest, streamed to disk, with the same size limits and replies as
`PUT .../files/...`. `path` is exactly the path the package lists: `library/<...>/<file>`
(at least three parts) or `thumbs/<name>.png`. Anything else is `400 bad_upload_path`.
These files live apart from files staged for `commit` and never reach the library except
through `ingest`.

**`POST /api/v1/uploads/{id}/ingest` (write)**, body `{ "package": <a package>, "onConflict": "skip" | "replace" }`
(at most 5000 items, body up to 32 MB):

```json
{ "items": [ { "id": "uid://sky1", "name": "Noon", "kind": "skybox", "outcome": "copied", "sourceRemoved": false } ],
  "revision": 136, "base": 135, "changes": { "…": "the rows the ingest wrote" } }
```

Each item is `copied`, `replaced` (only with `"replace"`), `skipped` (the id exists and the
default `"skip"` applies) or `failed` with a `reason`. The server first refuses any entry
whose `name`, `type`, `pack`, `category`, `file`, `files`, maps or `origin.files` are not plain
relative names, and any file path outside `library/` or `thumbs/`, then runs core's
`ingest_package`: every file is checked against its `size` and `sha256` as it is staged (a
file that does not match fails its item, nothing is written), items are committed in
groups through `merge_entries`, an id that belongs to a different item is refused, and
`transferOf` puts the item's own `pack` and `category` back. Shared thumbnails in the
package (PNG, at most 2 MB) are stored like `PUT /thumbs`. The `user` part of a package is
ignored. The upload is removed afterwards, whatever the outcome. A library the server
cannot write to answers `422 import_failed`.

### `POST /api/v1/assets/delete` (write)

`{ "ids": ["uid://sky1"] }` (raw ids of top-level items, at most 10000) →

```json
{ "revision": 137, "base": 136, "deleted": ["uid://sky1"], "changes": { "deleted": ["uid://sky1"], "…": "the /changes page for since=base" } }
```

A version 1 client gets `{ "revision": "…", "deleted": […], "catalog": { … the new catalog, as GET /catalog } }`.

This is `toybox-core`'s `delete_from_library`: the items, their variants and
the files only they use are removed, and so are their shared thumbnails.
Unknown ids make the whole request `404` with nothing deleted. The rows are
removed in one database transaction. (Core keeps
timestamped `catalog-before-delete-*.json` backups of the deleted entries and the
deleted derived rows beside the library database and the cache folder, and keeps the
newest 5 of each; an item's emptied set folder is
removed with its last file. Publication snapshots under
`_library_config/migration-backups/storage-*` are pruned the same way (the `0004-*` step 4 backup is never pruned), and both
that and a sweep of already-empty set folders also run once when the library is
first opened.)

### `POST /api/v1/assets/metadata` (write)

Change the **tags** and the **provenance** (source, publisher, license, web
address) of items:

```json
{ "edits": [ { "id": "uid://sky1",
               "tags": ["footstep", "Carpet"],
               "provenance": { "source": "Better Pack", "url": "https://example.com/pack", "license": null } } ] }
```

- `id` is the raw id of a top-level item or of one of its variants. `tags`, when
  present, replaces the item's tags (normalised: trimmed, lower-cased, de-duplicated,
  no colon, at most 64 of 48 characters; `[]` removes them all). `provenance`, when
  present, is partial: an absent field stays, `null` or `""` clears it, text sets it
  (at most 200 characters, `url` an `http(s)://` address of at most 2000). The
  importer's `filesystemPath` cannot be set; unknown fields are `400 bad_request`.
- At most 5000 edits and 32 MB. The request is all or nothing: an unknown id is `404
  not_found`, an invalid value `400 invalid_metadata`, an empty list `400 no_edits`, and
  nothing is written. A failure while writing is `422 metadata_failed`. A read token
  gets `403 forbidden`.

```json
{ "revision": 138, "base": 137, "updated": ["uid://sky1"], "touched": ["uid://sky1"],
  "changes": { "assets": [ { "…": "each changed top-level item as GET /catalog serves it, with its new tags" } ], "…": "" } }
```

`touched` lists the top-level items the edit changed (an edited variant counts for its parent). A
version 1 client gets `{ revision (string), updated, assets }` with the touched items themselves.
This is `toybox-core`'s `ops::metadata::update_entries`: under the write lock the
entries and their derived rows are rewritten in one database transaction (a backup of the
edited entries as they were, `_library_config/catalog-before-metadata-<stamp>.json`,
stays beside `library.db`; the newest ten are kept), and no file moves:
neither tags nor provenance are part of where an item's files are stored or of its
content fingerprint. Every client sees the change when it next follows `/changes`.

### Tokens (admin)

- `GET /api/v1/tokens` → `{ "tokens": [ { "id", "name", "role", "createdAt", "lastUsedAt" } ] }`.
  Times are seconds since the Unix epoch; `lastUsedAt` is `null` if never used
  (it is updated in memory and saved about every 30 seconds). Secrets and
  hashes are never returned.
- `POST /api/v1/tokens` `{ "name": "alice laptop", "role": "read" }` → `201`
  `{ id, name, role, createdAt, lastUsedAt, "token": "tbx_…" }`. **The token is
  in this one response only.** Names are 1 to 64 characters, unique ignoring
  case (`409 name_taken`).
- `DELETE /api/v1/tokens/{id}` → `204`. `409 last_admin` if it is the only
  admin token. The token stops working at once.

### Logging

One line per request on stdout (journald under systemd):
`ts=2026-11-17T09:30:00Z method=GET path="/api/v1/catalog" status=200 token="alice" ip=203.0.113.5 ms=3`.
Tokens (and the `Authorization` header) are never logged. `ms` is the time to
the response headers; a download continues after it.

## Writes are serialised

Imports, deletes, metadata edits and index rebuilds take one lock, so there is one writer at a
time; reads and downloads never wait for it. Core work runs on blocking
threads. A large import therefore delays another import, not browsing.
Thumbnail uploads do not take the lock (they touch only the cache folder and
are atomic). The database itself allows one writer at a time; readers (browsing,
`/changes`) do not block it.

## Security notes

- **Use HTTPS.** Tokens are sent in every request. `serve` listens on
  `127.0.0.1` by default and the installer puts Caddy in front, which gets and
  renews a certificate automatically. Do not expose port 8787 directly.
- Tokens are 256-bit random values; only their SHA-256 is stored, in
  `server.json` (mode 0600). A stolen `server.json` does not reveal tokens.
  Lose a token: revoke it and make another.
- **What a read token can see:** the whole catalog (names, packs, categories,
  import provenance without local paths, file lists) and every file under
  `library/`, including original source files kept with items. It cannot see
  `server.json`, `library.db`, backups, or staged uploads, and the server never
  returns its own folder names. If something must stay private, do not put it
  in the served library.
- A **write** token can add and delete items, edit their tags and sources, and
  replace shared thumbnails; it
  cannot write anywhere but through those routes (uploads are confined to
  `uploads/<id>/files/library/_import_*`, and committed entries may not name
  paths outside their own staging folder or choose storage paths).
- A wrong token costs the caller 400 ms, and 10 of them lock an address out for
  15 minutes; the lockout is in memory and a service restart clears it.
- The systemd unit runs as an unprivileged user with `ProtectSystem=strict` and
  write access to the data folder only, and drops all capabilities.
- There is no per-token library or folder scoping and no audit log beyond the
  request log.

## Backup

Everything is in the data folder. Copy `/var/lib/toybox` (the `cache/` and
`uploads/` folders may be skipped; they are rebuilt or temporary) with rsync,
a snapshot or a plain copy. The library database `library/_library_config/library.db`
runs in WAL mode, so it has `library.db-wal` and `library.db-shm` files beside it
while the service runs: copy all three together, or stop the service first. A copy taken
while an import or delete is running
can be inconsistent; core's own transaction journals in
`library/_library_config/` repair that on the next start, but a quiet moment is
better. To restore, put the folder back and start the service. The `library/`
folder inside is an ordinary Toybox library, so it can also be added to the
desktop app as a local library.

## Libraries made on Windows

A library folder made by the desktop app on Windows can be copied to the server
and back. The library holds no machine-specific paths: entries and the derived
rows in `library.db` use `/` and library-relative paths, and the migration journal is relative to
the library. The one exception is a *half-finished* storage change
(`_library_config/storage-pending.json`), which also records where the desktop
app kept its catalog cache. When that path is not a path on the machine now
reading it (a `C:\...` path on Linux, or the reverse), recovery finishes the
library's own files and leaves the cache to be rebuilt, instead of refusing to
open the library for ever. A cache path that is valid here but different is
still refused. Names that differ only in case, and names Windows cannot store,
cannot be uploaded (see above); if a library copied up from Windows relies on
Windows ignoring case (a glTF that names `Textures/A.png` for a file called
`textures/a.png`), those references do not resolve on Linux and have to be
fixed in the library.

## Limits and known gaps

| | |
| --- | --- |
| Library | one per server |
| Single file in an import | 2048 MB (`--max-file-mb`) |
| One import in total | 8192 MB (`--max-upload-mb`), at most 5000 files |
| Imports at once | 16; idle ones expire after 1 hour; none survive a restart |
| Commit / ingest | at most 5000 entries (items), body 32 MB |
| Thumbnail | 2 MB, PNG |
| Other JSON bodies | 2 MB |
| Delete | at most 10000 ids at a time |
| Metadata edit | at most 5000 items at a time, body 32 MB |
| Version 1 `GET /catalog` | built on the first request after a change and held in memory for that revision (tens of MB for tens of thousands of items); a client that uses `/changes` never causes it |

Not provided: user accounts, per-token scoping, rate limits other than the
sign-in lockout, TLS inside the server, compression (Caddy compresses the
catalog and thumbnail list), request timeouts (downloads and uploads may be
long), and live change notification (clients poll `/info` for `revision`, or `/changes`).

The library is read through `toybox-core`, so the first request after a start
(or after `library.db` was replaced or changed from outside) takes as long as opening the library in
the desktop app; the installer and `serve` do it up front. Core migrations run
when the library is opened, so serving an older library upgrades it in place
(a library that still has the JSON manifest is converted to `library.db`, see
[SQLite library storage](sqlite-storage.md)), and a library newer than the program
is refused. The server speaks API version 2; a current desktop app refuses a server
that speaks version 1.
