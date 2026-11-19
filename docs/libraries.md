# Libraries

Toybox shows any number of libraries together in one merged view. A library is
a folder in the managed layout described in [Library storage](library-storage.md);
nothing about that layout changed. This page is about how Toybox keeps track of
several of them. A library is either a folder on this computer (`kind: "local"`,
they may sit on different drives) or one served by a
[Toybox server](server.md) (`kind: "remote"`, see [Remote libraries](#remote-libraries)).
Both appear in the same merged view and behave the same wherever they can.

## The registry

`<app-data>/settings.json` holds the list:

```json
{
  "libraries": [
    { "id": "3f9c0a41de", "name": "Synty", "kind": "local", "root": "D:/dev/resources" },
    { "id": "a07be2c915", "name": "Archive", "kind": "local", "root": "E:/assets" },
    { "id": "5d21e07b3c", "name": "Studio assets", "kind": "remote", "url": "https://assets.example.com" }
  ],
  "defaultLibrary": "3f9c0a41de"
}
```

- `id` is a short random string made when the library is added. It is never
  derived from the path, so a library can move or be served from elsewhere
  without changing identity. Everything Toybox keeps about a library is keyed by it.
- `name` defaults to the folder name (a remote library: the server's name) and can
  be edited (Settings → Library).
- A remote entry has `url` instead of `root`. **The access token is never in
  `settings.json`**; see [Where the token lives](#where-the-token-lives).
- `defaultLibrary` is where imports go unless the user picks another library
  in the import screen's "Into library" box.
- Versions before several libraries stored one `libraryRoot`. On the first start
  it becomes the first library and the default (the id is made then),
  `libraryRoot` is no longer written, and saved exports that recorded that root
  are scoped by `libraryId` instead (their `lastAssets` fingerprints are keyed by
  the library's raw asset ids). The step is idempotent.

Settings → Library lists the libraries with name, folder (or server address), item
count, a "Default for imports" choice, **Add existing folder…**, **Create new…**,
**Add remote library…**, **Rescan** and **Remove…**. First run, with nothing registered, shows the
existing picker and adds the first library.

### Removing a library

Removing only edits the registry. It never touches the library folder or any
file in it (for a remote library: never the server). Toybox also **keeps** the library's app data
(`<app-data>/libraries/<id>/`, below). `user.db` there holds the user's own work
(favourites, personal tags, collections, preview choices) which can't be rebuilt from the
folder, and thumbnails are slow to regenerate; both are small next to the
library. `libraries/<id>/library.json` records the folder the data belongs to, and
**adding that folder again gets the old id back**, so the metadata and
thumbnails return. Nothing asks about it. The cost is some leftover disk use for
libraries that are never re-added; deleting `<app-data>/libraries/<id>` by hand
is safe for an unregistered id.

## Per-library app data

```text
<app-data>/
  settings.json
  libraries/
    <id>/
      user.db          this computer's personal data: favourites, personal tags,
                       collections, preview choices, import paths, thumbnail state
      library.db       remote libraries only: the replica of the server's library
      thumbs/          thumbnails, <sanitised asset id>.png
      library.json     { root, name }: lets a removed folder be recognised
      animation-preview-version   one-time refresh marker
  browse-index.db      derived, per machine, safe to delete (see sqlite-storage.md)
```

A local library's own data (`library.db`: items, derived assets, tags, content
hashes, rigs, change log) is in the library folder, `_library_config/library.db`,
not here. Libraries from before the databases (migration step 4) had
`catalog.json` (derived catalog plus personal data) and `content-hashes.json` in
this folder; the first open converts them (the old files are moved, not deleted,
into `_library_config/migration-backups/0004-<stamp>/`). The files, tables and
the migration are described in [SQLite library storage](sqlite-storage.md).

Before several libraries there was one `<app-data>/catalog.json` and one
`<app-data>/thumbs/`. At the first start after the settings migration they move
into the folder of the library that owns them (the registered library whose root
the old cache records). The move is crash-safe: thumbnails move first and the
cache last (it names the owner), every step is a rename that is skipped when its
source is gone, and nothing is overwritten (a colliding old copy is renamed
`*.legacy`). The cache moves unchanged, so favourites and collections
survive; the database conversion above then takes it from there. A step that
fails is retried at the next start.

Per-library state elsewhere follows the same rule: the migration journals live in the library's own `_library_config/`, the audio metadata cache
is keyed by canonical file path, and the one-time animation-preview refresh is a
marker file in the library's app-data folder rather than a global setting.

## The backend works on one library per call

Every command that used "the library root" takes `library: String` (an id) and
resolves it through one type, `LibraryHandle { id, kind, root, cache_path,
thumbs_dir }` (`toybox-core`'s `infra/registry.rs`, resolved by `infra::library::resolve` in the app, `Registry::resolve` in core). A
command never joins app-data paths itself. Commands that take asset ids take the
raw ids of that one library.

| Command | Notes |
| --- | --- |
| `list_libraries()` | `{ id, name, kind, root, available, canWrite, canEditFiles, isDefault, offline, offlineReason?, remote? }[]`. `available` is false while the library can't be read (an unplugged drive; a remote library that was never reachable and has nothing cached); it stays registered. `remote` is `{ url, role, serverName, offline }`, see [Capabilities](#capabilities). |
| `add_library(path, name?)` | validates like a library root; rejects a folder that is already registered. |
| `create_library(path, name?)` | scaffolds an empty library in an empty folder, then adds it. |
| `rename_library(library, name)`, `remove_library(library)`, `set_default_library(library)` | |
| `update_asset_metadata(library, edits)` | change the tags and provenance of items, see [Tags](#tags). Write access, local or remote. |
| `open_library(library, force)`, `scan_library(library, forceReseed)`, `delete_assets(library, assetIds)`, `merge_seed_entries(library, entries)` | `scan_library` is the explicit rescan (rebuilds the derived rows in `library.db`; it returns a summary, not a catalog). The window reads pages, counts and single assets from the databases through `query_assets` and the other browse commands, see [SQLite library storage](sqlite-storage.md#commands). `load_catalog`, `save_catalog` and `list_pending_thumbs` are no longer commands. |
| `resolve_asset_path(library, relPath)`, `library_root(library)`, `thumb_dir(library)`, `thumb_path(library, assetId)` | |
| `save_thumb`, `invalidate_thumb`, `set_thumb_state`, `pending_thumbs`, `clear_thumbs` | all take `library` first. |
| `transfer_assets(fromLibrary, toLibrary, ids, mode, onConflict, includeClips?)` | copy or move between two libraries, see Transferring. |
| `export_copy(library, req)`, `export_glb(library, req)`, `recenter_asset`, `convert_to_gltf`, `load_packs`, `compatible_previews`, `read_animation_preview`, `read_preview_source`, `read_preview_pair`, `read_asset_gltf`, `assemble_glb_for_asset`, `perform_asset_copy`, `preview_export`, `asset_fingerprints` | `library` first. |

The asset-protocol scope is extended to every registered local root at start-up
and when a library is added, and to the remote file cache (`<app-cache>/libraries`). The standalone tools in `src-tauri/crates/toybox-core/src/bin` still
take an explicit root and cache path (the cache path is the old `libraries/<id>/catalog.json` location: it names the folder that holds `user.db`, and a pre-database cache found there is converted).

## Crates

`src-tauri` is a Cargo workspace (the app package `toybox` is also the root, so
`tauri dev` / `tauri build` and `src-tauri/Cargo.lock` work as before) with three
members, `toybox-core`, `toybox-server` and `toybox-remote` (all in `src-tauri/crates`):

- **`toybox-core`** has no Tauri, webview or GUI dependency (check with
  `cargo tree -p toybox-core`). It holds the catalog model (`domain`), library
  storage, migrations, audio and animation import (`infra`), the registry data
  model and library folder rules (`infra::registry`, `infra::library`,
  `infra::settings`), and the operations that act on one library given a
  `LibraryHandle` (`ops`: catalog load/save, scan, merge of staged entries,
  delete, exports, thumbnails, previews, FBX promotion). The maintenance
  binaries `reindex`, `migrate-library` and `repair-colors` live here too.
  A headless server depends on this crate only.
- **`toybox`** (the app) keeps what needs an `AppHandle`: the app-data location,
  the live `LibraryState` and asset-protocol scope, the plugin host
  (`commands/plugins.rs`, `plugin_services.rs`, bundled plugins) and the Tauri
  command wrappers. A command resolves its library from the handle and calls the
  matching `toybox_core::ops` function; it contains no library logic of its own.

- **`toybox-remote`** is the client for remote libraries (`ureq` with rustls for HTTP,
  `keyring` for tokens; no GUI, no async runtime, no OpenSSL). It depends on
  `toybox-core` only and is tested against an in-process `toybox-server` (a
  dev-dependency; the desktop app itself has no tokio or axum). A separate crate,
  not a module of the app, because it has no use for Tauri and its tests need
  none of it.

A plain `cargo build` in `src-tauri` builds both (`default-members`), so the
three tools are still produced; `cargo run --bin migrate-library` works as
before. `cargo test --workspace --lib` runs the tests of both crates.

## Qualified ids at the API boundary

A raw asset id is only unique inside its library: importers derive stable ids
(`uid://import_<pack>_<category>_<stem>`), so two libraries can hold the same id.
The frontend keeps ONE merged catalog and keys everything (selection, previews,
favourites, thumbnails, grid keys) by `asset.id`, so inside the frontend ids are
**qualified**: `"<libraryId>::<rawId>"`, and every asset has `library: <id>`.

The translation is done in one place, `src/services/tauriApi.ts`, with the pure
functions in `src/domain/libraryId.ts`:

- A catalog arriving from the backend is stamped with `library` and its ids are
  qualified. References between assets are qualified with it: `id`, each
  variant's `id`, `user.previewAssetId` (on the asset and its variants), and an
  animation's `clip.sourceId`. The built-in preview ids (`toybox:...`) are not
  library items and are left alone.
- Calls that send ids group them by library and send raw ids with the library id
  (`deleteAssets`, `exportCopy`, `exportGlb`, `previewExport`, `assetFingerprints`,
  `clearThumbs`). Per-asset calls (`saveThumb`, `readAssetGltf`, `recenterAsset`,
  ...) take the qualified id and split it. Results that carry ids come back
  qualified.
- The window no longer holds or saves a whole catalog (`saveCatalog` is gone): it
  asks for pages and single assets, see [SQLite library storage](sqlite-storage.md#app-queries-build-step-4),
  and changes go through per-item commands. Qualified ids are never written to
  `library.db` or `user.db`.
- Path-based reads (`assetUrl(library, rel)`, `readPreviewSource(library, ...)`)
  take the asset's `library`. Previews, retargeting and rig matching only
  consider items of the asset's own library (`scopeCatalog`), because a preview
  reads files from one root.
- The merged catalog unions `rigs` (a rig id is a hash of its contents).

Plugins see the same: an `AssetView` has a qualified `id` and `library`, and
passes ids back to the host unchanged. `host.getLibraryRoot()` is the root of the
library the panel works in (the import's target library, or an asset panel's
asset's library), so importers and the Substance panel need no change. An asset
panel that commits an entry with `asset.id` replaces that item in its own
library: the host strips the qualifier. The import panel context also has an
optional `library?: { id, name }`.

## In the interface

- The filter panel has a **Libraries** group, first, shown when more than one
  library is registered. It selects like Sources (click, Shift-click) with live
  counts; `Filter.libraries` holds the selected ids.
- Asset properties show a **Library** row when there is more than one.
- The import screen has **Into library** (default library preselected) when
  there is more than one.
- A remote library has a small cloud mark next to its name (filter group, properties,
  settings) and, in Settings, a badge (**Remote · read-only**, **Remote · can edit**,
  **Remote · admin**) and an **Offline** badge with **Try again**.
- Delete and export work across libraries: the selection is grouped, each library
  acts on its own items, and the reports are merged. A library that can't be read
  is listed under the grid and the others still show.
- Saved exports belong to a library (`libraryId`). The export screen lists the
  setups of the first selected item's library (the default library when nothing
  is selected); the Exports manager lists all of them. Change tracking covers
  the setup's own library; items of other libraries in the same send are exported
  but not tracked.

## Transferring

Items can be copied or moved from one library to another. Select them, then use
**Copy to library…** or **Move to library…** (selection bar; the inspector has
the same two buttons for the item it shows). Both appear only when more than one
library is registered. The dialog asks for the target library (writable ones,
without the source when the selection comes from one library), what to do with an
item whose id already exists there (**Skip it**, the default, or **Replace it**),
shows `12 items from resources → Second`, then progress, then a report: copied /
replaced / skipped / failed, with the reason for each skipped or failed item. A
move asks for confirmation in the same dialog ("They will be removed from … after
they are safely copied."). A selection that spans libraries is handled per source
library; items already in the target are reported as skipped.

### Which libraries can be written

`list_libraries` reports `canWrite` for each library: true when the folder is
available and its `library.db` can be opened for writing (a read-only drive, share or
file, and an unreadable folder, are false; nothing is written to find out). For a
remote library it is true when the token's role is `write` or `admin` **and** the
server is reachable.
Transfer targets, the import screen's "Into library" box, Delete and the origin
alignment tools (recenter) are offered only for writable libraries, with the
reason in plain words ("Archive (it is read-only)") where something is held back.
Copying *out of* a read-only library is allowed; moving out of it is not, because
a move deletes from the source. The backend also refuses a read-only target and a
move out of a read-only source. A remote library with the `read` role (or one that
is offline) reports `canWrite: false` and is covered by the same checks, with its
own reasons: "your access token can only read", "the server cannot be reached right
now".

### The two halves and the package

`toybox_core::ops::transfer` (`ops/transfer.rs`) splits a transfer into an export
half and an ingest half joined by a neutral, serialisable **package**. Neither half
knows where the other lives, so a remote library can be on either side:

```text
export_package(source, ids, ExportOptions)                           -> TransferPackage
ingest_package(target, package, IngestOptions, read_file, progress)  -> TransferReport
transfer_between(source, target, ids, TransferOptions, progress)     -> TransferReport
                                                      (local to local; adds "move")
```

```text
TransferPackage { version, sourceLibrary, assets: PackageAsset[], failed: ItemFailure[] }
PackageAsset    { id, name, kind,
                  entry,    the item's entry exactly as the source records it (`items.entry`
                            in its library.db: variants, format, provenance, origin,
                            storagePath ... unchanged)
                  files,    [{ path, size, sha256 }]: every file of the item and its
                            variants, each once
                  user,     { <asset or variant id>: <personal metadata> } from the
                            source's user.db
                  thumbs }  [{ id, file: { path: "thumbs/<name>.png", size, sha256 } }]
```

The package holds no bytes. `read_file(path)` returns a reader for a package path:
`library/...` paths are library files, `thumbs/...` paths are thumbnails.
`local_reader(source)` serves a folder on this machine (confined to the library
root); a remote library would serve the same paths over HTTP. Ingest checks the
size and SHA-256 of every file as it stages it, so a file that changed after it was
described fails that item rather than landing damaged.

Mapped onto a remote library (implemented in `toybox_remote::transfer`, which handles
  every pairing of local and remote libraries; two local libraries use
  `transfer_between` as before):

- **Remote source** (download): `POST /api/v1/assets/export` makes the server run
  `export_package` over its own library and answer with the package; `read_file` becomes
  `GET /files/<path>` (and `GET /thumbs/<id>` for `thumbs/...` paths); the client then
  runs `ingest_package` into its local target. Needs the `read` role. The server's
  package carries no personal metadata; the client fills it in from its own `user.db` for
  the remote library, so favourites and tags follow between any two libraries on this
  computer.
- **Remote target** (upload): the client runs `export_package` over its local source (or
  receives one from a remote source), streams every file of the package to
  `PUT /uploads/<id>/package/<path>` and calls `POST /uploads/<id>/ingest`; the server
  runs `ingest_package` with `read_file` answered from the upload, after refusing any
  entry whose names would escape the staging folder. Needs the `write` role. Because
  it is the same code, ids, conflicts (skip / replace), `pack` and `category`
  (`transferOf`), variants, kept originals and thumbnails behave exactly as in a local
  copy, and every file is checked against its size and SHA-256 on arrival. The client
  never sends personal metadata; after the transfer it applies the source's favourites
  and tags to its own `user.db` for the remote library.
- **Move** is copy, verify, delete (`complete_move` in core, shared by all pairings).
  Verify checks the target's catalog; the delete half is the source library's normal
  delete (`ops::delete` locally, `POST /assets/delete` for a remote source, which
  therefore needs the `write` role on it). One item that cannot be removed does not
  keep the others.

### Ingest goes through the importer path

Ingest stages each item exactly as an importer would and commits through
`ops::import::merge_entries`, so storage keys (immutable set folders under
`library/<type>/`), shared `library/models/_resources/<digest>` images, validation,
the animation hook and the database update (one `Store::apply` transaction) are the import's own. Staging is
`library/_import_t_<hash of the item id>/...` (removed afterwards; images shared by
a group are staged once as `library/_import_t_res/<digest>.<ext>`). The staged entry
is the entry minus `storagePath`, with `pack`/`category` replaced by the
private staging ones; the real ones ride in a `transferOf` field that the storage
layer puts back before it writes the entry to `library.db`, so a transferred item keeps its
`pack` and `category` (an import normalises `pack` to `imported`). Items are
committed in groups of 20; if a group is rejected its items are committed one by
one, so only the bad one fails.

### What travels with each kind

| Kind | Files | Entry |
| --- | --- | --- |
| model | the `.gltf`, its single `.bin`, every external image (stored once per content in `_resources`), the kept original files in `origin.files` | all of it: `format`, `provenance`, `origin` (plugin, meta, files), `animationsExtracted`, ... |
| variants (detail levels, sizes) | each variant's own files, as above | each variant inside the parent's `variants` |
| material | every map file, the kept archive and files in `origin.files` (stored once per library; a transfer carries each distinct one once, and sends none the target already stores) | `maps`, `flipNormalGreen`, `origin`, ... |
| audio | the audio file | all |
| animation clip | its `.gltf` and `.bin` (clips are self-contained) | all, including `standalone` |
| plugin-defined type | every file in its `files[]` list | `type`, `files`, `meta`, ... |

An item brings its variants as a whole; a variant is not transferred on its own.
A glTF with several buffers, or whose URIs point outside the library, makes that
item fail (the storage layer refuses the same scenes). Two variants that keep
different files under the same name also fail, because both would be stored in one
folder.

**Animations extracted from a model are not dragged along by default.** A clip is an
item of its own (own id, file and metadata), so copying a model copies the model
(flagged `animationsExtracted`, so the target does not extract the clips again) and
leaves its clips behind. Tick *Also take the animations extracted from these models*
(`includeClips`) to take the clips whose source is the selected model or one of its
variants as well. A clip's `sourceId` (the model it was cut from) is written in its
glTF and is never rewritten: ids are kept, so the reference resolves exactly when the
target also has a model with that id (transferred in the same batch or earlier), and
otherwise points at nothing, as it does for a clip whose source was deleted. Clips
can also be transferred on their own by selecting them. Moving a model without its
clips deletes the model from the source and leaves the clips there.

### Ids, conflicts and what is kept

Raw ids are kept. If the target already has an item with the same id, the default is
**skip**: reported as skipped, nothing is written, and a move leaves the source item
alone. **Replace** overwrites it in place through the same upsert an import uses (a
replaced material or plugin item's old folder is retired; a model's old set folder is
left behind like on any re-import). If the id of the item, or of one of its variants,
belongs to a different item of the target, that item fails with a reason and nothing
is overwritten.

The entry's provenance and the [tags](#tags) travel in the entry (also into and out of a server). Personal metadata (favourites,
collections, the preview choice, the filesystem provenance) lives in the per-library
`user.db`, not in the library: for an item that is new to the target it is copied from the
source's `user.db` (a preview choice naming an item the target lacks is dropped); for a
replaced item the target keeps its own. Rendered thumbnails are copied too, so the
target does not re-render them.

### Move

Move is copy, then, only for the items that were copied or replaced and are verified
in the target (recorded in its `library.db`, every file present), a delete from the
source through the normal delete path. Skipped and failed items stay where they are;
an item whose copy failed is never deleted. If the batch delete is rejected, the items
are deleted one by one, and one that cannot be removed is reported ("Copied, but not
removed from the source") while the target keeps its copy.

### Command

`transfer_assets(fromLibrary, toLibrary, ids, mode: "copy" | "move", onConflict: "skip" | "replace", includeClips?)`
takes raw ids of the source library and returns
`{ items: [{ id, name, kind, outcome: "copied" | "replaced" | "skipped" | "failed", reason?, sourceRemoved }] }`.
It emits `transfer-progress` events `{ fromLibrary, toLibrary, done, total }`. In
`tauriApi.ts`, `transferAssets({ assets, toLibrary, mode, onConflict, includeClips?, onProgress? }, targetName)`
takes qualified assets, groups them by source library and returns one merged report.
The frontend then re-reads only the libraries named by `affectedLibraries` (the
target, and the sources after a move), without a rescan.

## Remote libraries

A remote library is a library served by [`toybox-server`](server.md). The server
holds the library (items and their files); this computer keeps what is personal to
it (favourites, collections, preview choices, in `user.db`), keeps a replica of the
server's library rows (`library.db`) and caches the files it has fetched.
To the rest of the app it looks like a local library: it has an id, its ids are
qualified like any other, its items sit in the merged catalog and the same commands
work on it. The seam is `LibraryHandle`: for a remote library `root` is a local
**file cache** with the server's layout (`<app-cache>/libraries/<id>/files/library/...`),
`cache_path` and `thumbs_dir` are the usual `<app-data>/libraries/<id>/...`, and the
handle carries a `RemoteLink` (`FileProvider`) that fetches what is missing.

### Adding one

**Add remote library…** asks for a *server address* and an *access token*. **Connect**
calls `GET /api/v1/info` and shows "Connected to <server name> as <role>"; **Add
library** saves it. The library is named after the server unless renamed.

- The address is normalised (`toybox-remote/src/url.rs`, mirrored in
  `src/domain/remoteUrl.ts`): a missing scheme means `https://`, a trailing slash or
  `/api/v1` is dropped, a user name or password in the address is refused.
  `https://` is always accepted. Plain `http://` only for this computer (`localhost`,
  `127.0.0.1`, `[::1]`) and private networks (`10.0.0.0/8`, `172.16.0.0/12`,
  `192.168.0.0/16`, `*.local`), with a warning that the connection is not encrypted.
  Anything else over `http://` is refused.
- Commands: `probe_remote_library(address, token)` (nothing saved),
  `add_remote_library(address, token, name?)`, `change_remote_token(library, token)`
  (checked with the server before it replaces the old one), `reconnect_remote_library(library)`
  ("Try again"), `remote_cache_size`, `clear_remote_cache`, `ensure_asset_files`,
  `list_remote_tokens`, `create_remote_token`, `revoke_remote_token`.

### Where the token lives

In the operating system's credential store (Windows Credential Manager, macOS
Keychain, the Secret Service on Linux) through the `keyring` crate: service
`Toybox`, account = the library id. It is never written to `settings.json`, the
per-library `remote.json`, the replica databases, logs or error messages (errors name
the server address only, and debug output of a client hides it). If the credential
store does not work, adding a library fails with a message saying so rather than
falling back to a file, and an already added library reports it as the reason it is
unavailable. Removing a library deletes its token from the store and its downloaded
files; changing the token replaces it. The store sits behind a small trait
(`SecretStore`), so the tests use an in-memory one and never touch the real store.

### The catalog

`open_library` asks the server (`scan_library` and `force` start again with a full
download): `GET /info` for the role and server name, then `GET /changes` from the
cursor the replica holds, until the server says there is nothing more. Each page
is applied to the replica (`library.db` in `<app-data>/libraries/<id>/`) as one
transaction; the client's own favourites, collections and preview choices are
in `user.db` and are not touched by it. The library's `libraryRoot` is the file
cache. Tags the server derives from import provenance arrive with the items, and
so do the library's own `tags`; nothing personal is ever sent to the server. The
server is version 2 of the protocol: an older server is refused with a message
and the library shows as offline with that reason. See "Server protocol" in
[SQLite library storage](sqlite-storage.md#server-protocol-api-version-2) and
[Server](server.md). `<app-data>/libraries/<id>/remote.json` remembers the server
name, the last known role and the shared-thumbnail versions (not the token). A replica
from before the databases (a `catalog.json` in that folder) is converted on the
next open, and its personal data is kept in `user.db`.

**Offline.** If the server cannot be reached (no network, refused, TLS failure,
time-out) or no longer accepts the token, the replica is used and the
library is reported `available: true, offline: true` with `offlineReason`; it is
read-only until a request succeeds (**Try again** or the next load). With nothing
in the replica it is a plain error instead of an empty library.

### Files

Files are downloaded on demand into the file cache, to a temporary name that is
renamed into place once the size matches what the server announced. Two requests
for the same file wait for one download. Storage paths are content addressed, so a
cached file never goes stale and nothing is evicted; Settings shows the size of the
downloaded files per remote library and **Clear downloaded files** removes them (the
catalog, thumbnails and favourites stay; an import being prepared is not touched).

What is fetched, and where:

| Call | What it makes sure is on disk |
| --- | --- |
| `resolve_asset_path(library, rel)` (the viewer, audio, material and plugin file URLs all go through it) | `rel` **and the rest of the item it belongs to**, so opening a model brings its `.gltf`, `.bin` and every image in one request instead of one round trip each |
| `read_preview_source` / `read_preview_pair` | the glTF and the buffer and images it names (read from the file itself) |
| `export_copy`, `export_glb`, `preview_export`, `asset_fingerprints`, `read_asset_gltf`, `assemble_glb_for_asset`, `perform_asset_copy` | the content files of the requested items and their variants (core's `collect_assets`) |
| `read_animation_preview` | the content files of the animation and of the model it is shown on |
| `ensure_asset_files(library, ids, includeSources)` (asset panels) | the same, plus with `includeSources` the source files kept in `origin.files` |
| `transfer_assets` from a remote library | nothing: files stream from the server into the target |

A model's file set is read from the item's asset row (`fileset.gltf`, `fileset.bin`,
`fileset.textures`), a material's from its maps, an audio item's from its file, a
plugin item's from `files`; the source files an importer kept (`origin.files`) are
not downloaded just to view or export an item. Commands that may download run on
background threads (`resolve_asset_path` is asynchronous), so the viewer shows its
usual loading state and the window never waits.

### Thumbnails

`GET /thumbs` lists the items with a shared picture. Those count as ready without
being downloaded: the picture is fetched into the local thumbnail folder the first
time `thumb_path` is asked for it, and dropped and fetched again when the server's
version changes. Only items without a shared picture are queued for rendering. A
picture this client renders itself is offered with `PUT /thumbs/<id>` in the
background when the role is `write` or `admin` (a failure is ignored; a `read` token
never uploads). A client with a read-only token therefore renders (and so
downloads the model for) every item the server has no picture for.

### Capabilities

| Flag | Local library | Remote library |
| --- | --- | --- |
| `available` | the folder can be read | online, or offline with a replica |
| `canWrite` | `library.db` can be opened for writing | role `write`/`admin` **and** online |
| `canEditFiles` | same as `canWrite` | always false |
| `remote` | absent | `{ url, role, serverName, offline }` |

Import, delete, editing tags and sources, and transfer-into follow `canWrite`; origin alignment (recenter) and
other tools that rewrite files in place follow `canEditFiles`, and a button that is
disabled shows its reason: "your access token can only read", "the server cannot be
reached right now", "files of a remote library cannot be edited in place yet; copy
the item to a local library to do that". The backend refuses these too.

### Writing

- **Import.** An importer stages files with `host.getLibraryRoot()`, which for a remote
  library is the file cache root (staging lives in `library/_import_<name>/`, which is
  never confused with a download: the server serves no `_import_` paths and "Clear
  downloaded files" leaves them alone). `merge_seed_entries(library, entries)` then
  uploads every staged file of the entries' packs (`POST /uploads`, one streamed
  `PUT .../files/<path>` per file, progress events `library-progress`), commits the
  entries (`POST .../commit`), removes the local staging whether it worked or not,
  cancels the upload on the server, and applies the `changes` the commit returned to the replica. Entries the server
  refuses are named with the reason (the others are in the library); the import
  screen shows the message and refreshes the library.
- **Plugin asset panels that commit** (the Substance re-render) use the same path:
  before the panel appears the item's files, including the source archive, are
  downloaded ("Downloading this item's files…"), and `commit` goes through
  `merge_seed_entries`. With a token that cannot write the panel says so up front.
- **Delete.** `POST /assets/delete`, then the `changes` in the reply are applied to the
  replica and the deleted items' files that no remaining item uses, their thumbnails and the preview
  choices that named them are removed locally.
- **Library tags and sources.** `POST /assets/metadata` (write role), then the replica
  is updated from the answer, see [Tags](#tags).
- **Transfer** in every direction, see [Transferring](#transferring).

### Access management

For an `admin` token a remote row has **Manage access…**: the server's tokens (name,
role, created, last used), **New token** (name and role; the secret is shown once
with a **Copy** button and a plain warning that it cannot be looked up again) and
**Revoke…** with a confirmation. The server's rule that the last admin token cannot
be revoked is shown as the server states it. **Change token…** (any role) replaces
the token this computer uses.

## Tags

There is one kind of tag: a descriptive keyword (`footstep`, `carpet`, `loop`, `ui`,
`coin`) that belongs to the asset and is **stored in the library where the asset's file
lives**, in its entry (`items.entry` in `_library_config/library.db`, field `tags`; the
`asset_tags` table indexes them). It reaches the
catalog as `Asset.tags`, is seen by everyone who has the library (**including everyone
who connects to a server** that serves it), is set by importers (`tags` on a seed entry)
or by anyone who can write the library, and travels with the item when it is copied or
moved to another library. There is no tag that stays on one computer.

(`Asset.user.tags`, in this computer's `user.db` (`user_tags`), still exists, but only as the internal
home of the `source: …` / `publisher: …` / `license: …` tags derived from provenance,
which feed the Sources, Publishers and Licenses groups and are never shown as tags. See
[Tags older versions kept on this computer](#tags-older-versions-kept-on-this-computer).)

`tags` is `string[]` on every entry (model, material, animation, audio, plugin
types) and on its variants. It is optional and skipped when empty, so old entries load
unchanged. The backend normalises it whenever it reads or writes an entry: whitespace
trimmed and collapsed, lower-cased, empties and duplicates dropped, order kept; a tag
containing a colon is dropped so it can never be mistaken for the `source: …` /
`publisher: …` / `license: …` tags derived from provenance; at most 64 tags of 48
characters (the rest is cut, not an error). The server holds no personal metadata
(favourites, collections, `user.tags` are in a client's `user.db`) and serves `tags`
with every asset, so a read-only client sees them.

They are descriptive data only: they are not part of the content fingerprint
(`contentHash`), and they are **not part of the key an item's files are stored under**.
`infra::storage` hashes the entry JSON into the folder key when it first stores an item;
`tags` are left out of that hash (and of each variant's), so editing tags never moves an
item's files and importing or copying the same item with other tags lands in the same
folder. `provenance` is still part of that hash for a first store (an item that is
already stored is never keyed again, so editing its provenance later moves nothing).

Search (the text box) and the **Tags** filter group use `Asset.tags` together with
whatever `user.tags` holds; the `source:`/`publisher:`/`license:` tags stay out of the
Tags group as before.

### Editing tags in the interface

The properties panel lists the asset's tags as plain chips, each with a small remove
button, and one field, "Add a tag". Adding or removing a tag saves it in the library
that holds the asset (a remote library through its server) and the chips update from the
answer; nothing is rescanned. There is no choice of where a tag goes.

Several assets selected: the selection bar has **Tag…**. The dialog takes tags to add
(separated by commas) and lists the tags present on the selection with how many of the
selected assets have each (**all**, or "3 of 8"); **Remove from all** marks one for
removal. **Apply** saves the whole change, with a progress bar for a large selection,
and ends with a plain per-library result.

#### Duplicates

An asset that exists in several libraries (see
[Duplicates across libraries](#duplicates-across-libraries)) is one asset to the user,
so its tags are too:

- The tags shown are the **union of the tags of all its copies** (the copy shown first,
  then any the others add). Copies whose tags had drifted apart are therefore shown the
  same way.
- Adding or removing a tag writes the **same full list to every copy**, in each library
  that holds one, which converges the copies. A copy that already holds exactly that set
  is not written. Edits are grouped per library, one `update_asset_metadata` call per
  library (split into calls of at most 5000 edits, the backend's limit), a remote
  library through its server. This is `src/domain/tagEditing.ts` (planning, pure) and
  `src/services/tagEdits.ts` (running).
- A library that fails does not stop the others. What happened is said per library, in
  plain words, e.g. "Saved in resources. Not saved in toybox.fornara.io: the server
  cannot be reached right now." The panel shows such a message under the tags; the
  multi-select dialog always shows its result.

#### When a library cannot be written

A copy in a read-only folder, behind a read-only token or on a server that cannot be
reached is not edited; the others are, and the result names it. The tag editor of an
asset is usable as long as **at least one copy can be written**. When no copy can be
written (for example a read-only library with no duplicate) the field and the remove
buttons are disabled and a plain sentence says why ("This library is read-only, so tags
cannot be changed."). There is no fallback that keeps the tag on this computer.

#### Tags older versions kept on this computer

Earlier versions could keep a tag only in this computer's cache (`user.tags`; now the
`user_tags` table of `user.db`). Those move into the library once:

- For a **local** library the desktop app does it when the library is opened
  (`ops::metadata::load_catalog_migrating`, `migrate_personal_tags`). Every `user.tags`
  entry that is not a `source:`/`publisher:`/`license:` tag, of an item or a variant the
  library holds, is added to the item's `tags` through the same `update_entries` that
  edits make (so with its backup of the edited entries), and removed from `user.tags`. A tag
  the item already has is only removed from `user.tags`. A tag that cannot be a library
  tag (it has a colon) or does not fit (the item already has 64 tags) stays where it was,
  and so does every tag of a library that cannot be written; those are still shown (as
  chips without a remove button) and found by search. It is idempotent, the check is one
  pass over the catalog just read (no write when there is nothing to move) and it never
  fails the load: a problem goes to stderr and the tags stay. Once a library has nothing
  left to move, `library.db` records it (`meta` key `personalTagsMigrated`) and later
  opens skip the check.
- For a **remote** library the client does it after loading, for libraries it can write
  (`migratePersonalTagsOfRemoteLibraries`): one `update_asset_metadata` per library, then
  the tags are removed from `user.db`. Variants of remote items are left as they are
  (shown, read-only). A failure is silent and tried again at the next load.

### Editing library metadata

`update_asset_metadata(library, edits)` changes the **library** tags and provenance of
items, locally or on a server:

```text
edits: [{ id,                     raw id of a top-level item or one of its variants
          tags?:        string[], replaces the item's tags ([] removes them)
          provenance?:  { source?, publisher?, license?, url? } }]   partial
```

A provenance field that is absent stays; `null` or `""` clears it; text sets it (single
line, at most 200 characters; `url` must be `http://` or `https://`, at most 2000). The
importer's `filesystemPath` is private to its computer and cannot be edited. The
request is all or nothing: an unknown id or an invalid value rejects it before anything
is written. It answers with the touched top-level items (an edited variant comes inside
its parent) as this computer's databases hold them.

In `toybox-core` this is `ops::metadata::update_entries(handle, &[MetadataEdit]) ->
Catalog`. It takes the same storage lock a publication takes (after finishing an
interrupted one) and writes the edited entries and their derived asset rows in one
database transaction (`Store::apply`), so **no rescan is needed** and only the rows
of the edited items are touched. A copy of the edited entries as they were stays in
`_library_config/catalog-before-metadata-<stamp>.json` (`{ "assets": [...] }`); the
newest ten are kept. The derived rows follow a provenance change the way a rescan would:
`user.provenance` and the provenance-derived `source: …` tags change, the importer's
path and the user's own tags stay. Files are never touched. Refused: a remote handle
(use the server), and a library whose `library.db` cannot be opened for writing.

On a remote library the same command calls the server's
[`POST /api/v1/assets/metadata`](server.md#post-apiv1assetsmetadata-write) (write role),
then applies the `changes` in the answer to the replica; it is refused with the usual
sentences for a read-only token or an offline server (the same gate as delete and
import). If the answer does not cover everything that happened meanwhile, the client
pulls the rest from `GET /changes`.

## Duplicates across libraries

Copying an item to another library keeps its raw id, but the same item also turns
up under other ids (imported again, from another pack of the same files). The view
recognises a copy **by its content**, not by its id, and shows it **once**. The id
stays what it is, because favourites, variants, clips, saved exports and exported
projects refer to it; a separate fingerprint, `contentHash`, is added.

### The fingerprint

`contentHash` is a lower-case hex SHA-256 over what the item *is*: byte-identical
files give the same hash whatever the id, name, pack, category, provenance, file
names or storage location. It is defined in `toybox-core`'s `ops/content_hash.rs`
and starts with a versioned, domain-separated preimage
(`toybox-content-v1\n<type>\n`), so the scheme can change later.

| Type | What is hashed |
| --- | --- |
| model, animation | the glTF JSON, normalised: every `uri` that names a file is replaced by that file's SHA-256 (so the `.bin` and every external image count, whatever they are called), embedded `data:` URIs stay as they are, and Toybox's own identity records are removed (below); nothing else is touched. A clip that is not a standalone file also hashes its clip index |
| material | one line per map `(role, channel, invert, SHA-256 of the file)`, sorted, so the order of the maps does not matter, plus `flipNormalGreen` |
| audio | the bytes of the file |
| plugin-defined type | the SHA-256 of each of `files`, in order |

The kept originals (`origin.files`: an `.fbx`, a `.sbsar`) never count, and a
variant has a hash of its own. Storing a scene rewrites its `uri`s (the buffer
becomes `<stem>.bin`, an image `../_resources/<hash>.<ext>`), which is why those
are replaced by content digests. Publishing also records, inside the stored glTF,
`extras.toyboxSourcePath` (the **staging path of the import, which contains the
file name**, and is what makes two imports of one file differ), and an extracted
clip carries `extras.toyboxAnimation.id`, `.clip.sourceId` and `.clip.sourceName`
(ids and names). Those are removed before hashing; everything else in the scene,
including node and animation names and the joints of a clip, is content.

### Where it is kept

Not in the item's entry. A stored file set lives in
a content-addressed folder (`library/<type>/<key>/...`; the key changes whenever
the id, the entry or any file does), so a table maps that key to the hash:
`hashes (key, hash)` in the library's own `library.db`, for example
`models/3f9c...(64 hex)` to `9a41...(64 hex)`. It was `content-hashes.json` (a
`{ "scheme": 1, "hashes": { ... } }` document) before migration step 4, which moved it
into the database. A local library keeps it in `<library>/_library_config/library.db`;
a server in its own library database. The `hashScheme` and `hashRevision` entries of
`meta` hold the scheme and a counter of changes to the hashes.
An item whose key is in it is never read again; a different scheme counts as empty,
so changing the definition starts over. The asset rows are filled from it whenever a
library is opened, scanned, imported into or deleted from (`assets.content_hash`, null
while unknown; the stored asset JSON never carries the field, it is added when an
asset is read). Stale keys (of deleted or replaced items) are
harmless and are pruned when a backfill starts, only when their folder is gone too.
The only operation that edits a stored file in place, origin alignment, recomputes the
hash of the item it changed. An item outside a content-addressed folder is not
fingerprinted (nothing says its files stay the same).

### Computing it without waiting for it

- **New items** are fingerprinted by the import that publishes them (`merge_entries`,
  which an import, a transfer into the library and a server upload all go through),
  from files that were just read anyway.
- **A library that was never fingerprinted** (the first start after this feature)
  needs one pass over every file. The desktop app starts it after the catalogs are
  loaded and shown (`start_fingerprinting`), on a background thread, one local library
  at a time, two files at a time, saving the hashes to `library.db` every 250 items so an
  interrupted pass resumes. Items with missing or unreadable files are skipped, not
  failed. Shared `_resources` images are read once per pass. While it runs
  Settings -> Library shows one line in that library's expanded panel,
  "Fingerprinting items for duplicate detection… 3,200 of 12,931"; nothing is shown in
  the main window.
- The backend reports with two events: `fingerprint-progress`
  `{ library, done, total, running }` and `fingerprint-hashes`
  `{ library, hashes: { <raw asset id>: <hash> } }` (variants included), the second
  every few hundred items and when the library is done. The frontend applies them to
  the items it holds (`applyContentHashes`, `patchCachedHashes`) and the grouping below
  updates without a rescan. `fingerprint_status` returns where
  a pass is, for a window that missed the events.
- **Remote libraries** are not hashed by the client: the server fingerprints and
  sends `contentHash` with every asset in its [changes feed](server.md#get-apiv1changessincerevisionlimitn-read).

### The rule

Two assets in **different** libraries are the same item when their type matches and
their `contentHash` matches. If either has no hash yet (the pass is still running, or
an older server) that pair falls back to type + raw id. Two assets of the **same**
library are never merged, however alike: kits ship identical models under different
ids on purpose.

One copy of each group is shown, chosen by `src/domain/duplicates.ts`. Libraries are
walked in preference order:

1. a library that can be read comes before one that is offline or unavailable;
2. then the **default** library, then the other local libraries in registry order,
   then remote libraries in registry order.

An asset is hidden when a library that comes before its own already shows a matching
one. If that library shows several that match (identical content under several ids),
the hidden copy is listed under the one with the same raw id if there is one, otherwise
the first. The same raw id in two libraries with **different** known hashes is two
items: both are shown, and each says so in its Library row ("resources · a different
version is in toybox.fornara.io").

De-duplication is derived, never stored: no field is added to an asset, so nothing
of it reaches `library.db`, and every copy keeps its own favourites and
collections (the shown copy's are the ones you see). Tags are the exception: they are
shown as the union of all copies and an edit is written to every copy, see
[Tags](#duplicates). It happens at one point,
`LibraryView`, and everything downstream uses that list: the grid, the type counts
in the sidebar, the toolbar "N of M", and the Sources, Publishers, Licenses,
Formats, Tags and Category counts all count a duplicated item once. It is a single
O(n) pass, memoised (`state/duplicates.ts`).

The **Libraries** filter is the way to see one library on its own. Its counts stay
the true number of items each library holds, and when libraries are selected the
grid shows *those libraries' own copies* (de-duplicated among the selected ones,
same preference). A selected or previewed copy that stops being the one shown is
dropped from the selection.

- Properties show the Library row as "resources · also in Studio assets" (remote
  libraries with their cloud mark), plus "· a different version is in …" when another
  library holds the same id with other content. Grid cards are unchanged.
- Export, delete and transfer act on the copy shown. Deleting removes it from that
  library only, the other copy then appears, and the delete dialog says so.
- The transfer dialog does not offer a library that already has every selected
  item (listed under "Not offered" as "already there"); a library that has only
  some of them still reports the rest as skipped.

## Known limits

- Exporting items of several libraries into one folder runs once per library.
  The per-library name-collision guard does not see the other libraries, so equal
  output paths overwrite each other; the merged report warns when that happened.
- A library's folder can't be moved in place; remove it and add the new folder
  (its personal data, in `user.db`, comes back if it is kept and the folder is the same one).
- Remote libraries: files are fetched whole (the server's `Range` support is not
  used), there is no cache size limit or eviction, no live change notification (a
  remote library is refreshed when it is loaded or rescanned), and the TLS roots are
  the ones built into the client (a server with a private certificate authority is
  not supported yet).
- Opening a remote library at start-up waits for its server, at most ten seconds to
  connect, before falling back to the replica.
