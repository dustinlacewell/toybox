# AI guide: migrate an asset library to managed Toybox storage

Use this guide for Toybox 0.4.1's storage migration. Read the implementation in
the checkout you are using before applying it to a later format version.

This document is procedural guidance, not authorization to modify an arbitrary
library. Establish the intended root/cache paths and the user's migration scope.
Do not migrate the library merely because you were asked to write or review a guide.

The task is to relocate existing file sets and preserve their identities and
metadata. Run Toybox's migration tool. Do not recreate the migration with folder
renames, filename classification, or a newly generated catalog.

## 1. Identify the input and the requested scope

An existing Toybox library has an authoritative manifest at
`<library-root>/_library_config/catalog.json`. Its derived catalog normally lives
outside that directory, in the user's Toybox app-data directory.

Distinguish these cases:

| Input | Procedure |
| --- | --- |
| Existing Toybox manifest and matching cache | Migrate both with the native helper. Preserve cached user data. |
| Existing Toybox manifest, no cache | Migrate the manifest and media. Use the intended cache path; the helper does not create a derived cache. |
| Cache exists but the source manifest is missing | Recover the source manifest from an existing backup before proceeding. Do not substitute the cache for the manifest. |
| Foreign asset collection with no Toybox manifest | Import through Toybox's plugins, or prepare an isolated inbound library and then run the migration helper. This is conversion/import work, not an existing Toybox format upgrade. |

For foreign collections, the model conversion details in
[the legacy conversion spec](library-format-spec.md) can help prepare inbound
files. Its pack-folder layout is legacy input, not the final managed layout.
Keep the original collection intact. Preserve existing identities when present;
where none exist, assign identities once rather than using basenames as keys.
Record supplied source, publisher, URL, license, and filesystem provenance.
Do not guess a license or publisher from a filename or pack resemblance.

Unless the user separately requests it, exclude metadata enrichment, texture
repair, format conversion of existing Toybox media, variant regrouping, rig
inference, animation extraction, thumbnail regeneration, and reindexing.

## 2. Understand the documents and version numbers

These numbers describe different things. Do not copy one into another.

| Document or setting | Meaning in 0.4.1 |
| --- | --- |
| Application version | `0.4.1`, the first release with this managed-storage upgrade. |
| `_library_config/migrations.json` → `version` | Durable migration version; completed storage migration is step **3**. |
| `_library_config/catalog.json` → `storageVersion` | Managed media layout version **1**. |
| Source manifest → `schemaVersion` | Legacy source-schema field, commonly `1`; the storage migration does not replace it with `3` or `5`. |
| App-data `catalog.json` → `schemaVersion` | Derived-cache schema; current code uses **5**. Migration-only work can leave an older cache schema unchanged. |

The source manifest registers assets and their file locations. The app-data
catalog contains resolved file sets, rig information, cached audio facts, user
tags, favorites, collections, and preview-model choices. It is derived data, but
it can contain user information that must survive. Never discard it to force a
rescan during a preservation-only migration.

Settings and thumbnails are separate app-data files. Preserve settings, saved
exports, plugins, and thumbnail files. They do not need new asset identities.
`filesystemPath` in provenance records the original source location; it is not
an active media reference. Do not replace it with the new storage path.

## 3. Find and record the actual paths

On Windows, the usual app-data directory is `%APPDATA%\com.toybox.app`.
`settings.json` stores the configured root as `libraryRoot`; the matching cache
is `catalog.json` in that app-data directory. On other systems, locate Toybox's
Tauri app-data directory for identifier `com.toybox.app` and verify its contents.
Do not reuse another person's username, drive letter, or library path.

Example discovery in PowerShell:

```powershell
$toyboxData = Join-Path $env:APPDATA 'com.toybox.app'
$toyboxSettings = Get-Content -Raw -LiteralPath (Join-Path $toyboxData 'settings.json') | ConvertFrom-Json
$toyboxRoot = $toyboxSettings.libraryRoot
$toyboxCache = Join-Path $toyboxData 'catalog.json'
Get-Item -LiteralPath (Join-Path $toyboxRoot '_library_config/catalog.json')
```

Resolve the root to an absolute path. Confirm that a supplied cache's
`libraryRoot` refers to this same directory. The helper deliberately leaves a
cache for another library untouched; `validatedCachedItems: null` is not proof
that the intended user's cache was migrated.

## 4. Prepare a migration-only run

1. Close Toybox and stop its development watcher if one is running. Coordinate
   only the relevant processes; do not kill unrelated apps or broadly terminate
   all Node, Cargo, or Explorer processes.
2. Close or navigate away from Explorer windows showing old source folders.
   Windows can otherwise block the final archive rename.
3. Save byte-for-byte copies of the source manifest, matching cache when present,
   settings, and existing migration journal to a separate backup directory
   outside `library/`. Record IDs, counts by kind, variants, and existing user
   metadata for comparison. Use a full media backup if complete rollback is a
   requirement; JSON snapshots alone are not a full library backup.
4. Keep enough free space on the library volume for verified copies of the
   indexed media while originals still exist. Do not delete originals to free
   space halfway through the operation.
5. Check for `_library_config/migration-pending.json` and
   `_library_config/storage-pending.json`. An existing marker calls for recovery
   through the same helper with the original root/cache paths. Preserve it.

Use a trusted checkout of `v0.4.1` or compatible newer code, current stable Rust,
and the platform's Tauri build prerequisites. The offline helper is a source-build
tool; do not assume the desktop installer supplies a `toybox --migrate` command.
If the checkout needs generated frontend/plugin resources, prepare them with
`pnpm install --frozen-lockfile` and `pnpm build`. Neither command launches the UI.
Do not start `pnpm tauri dev` while preparing the migration.

## 5. Run the native migration

From the repository root, using the paths discovered above:

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --bin migrate-library -- $toyboxRoot $toyboxCache
if ($LASTEXITCODE -ne 0) { throw 'Migration did not complete; preserve its journal and investigate.' }
```

Equivalent command with explicit paths on any supported shell:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --bin migrate-library -- \
  "/absolute/library-root" "/absolute/toybox-app-data/catalog.json"
```

The helper performs ordered migrations, resumes interrupted work, validates the
source entries, and validates a matching existing cache against the native
catalog types. It does not launch Toybox, rebuild catalog facts, extract clips,
or research metadata. A missing cache remains absent.

Its JSON report includes `storageVersion`, `sourceEntries`, `validatedCachedItems`,
`counts`, and `backupDirectory`. Counts are observations, not target numbers to
hardcode. Variants live within parent records, and older animation source entries
can differ from cached clip rows, so source-entry and cache-item totals need not
be equal.

Opening Toybox 0.4.1 also runs migrations. Normal app startup may subsequently
rebuild an outdated derived cache. When the task forbids deriving new facts,
use the offline helper and leave app startup/reindexing for a separate task.

## 6. Recognize the resulting storage contract

```text
<library-root>/
  _library_config/
    catalog.json
    migrations.json
    migration-backups/
  library/
    models/<opaque-set-key>/Name.gltf
    models/<opaque-set-key>/Name.bin
    models/_resources/<content-hash>.<extension>
    materials/<opaque-set-key>/<map files>
    animations/<opaque-set-key>/Name.gltf
    animations/<opaque-set-key>/Name.bin
    audio/<opaque-set-key>/<audio file>
```

The per-file-set directories isolate dependencies and filename collisions. They
are implementation details, not source groups. Source, publisher, license,
category, and tags organize the library through metadata.

The migrator owns key generation. Do not regenerate asset IDs or substitute a
folder's opaque key for its asset ID. The `pack` field remains for plugin and
export compatibility; retaining it does not make it a storage location.

In the **source manifest**, each primary and variant entry has `storagePath`:

| Kind | `storagePath` points to | Other declarations retained |
| --- | --- | --- |
| `model` | Its `.gltf` under `library/models/<key>/` | `id`, `file`, provenance, category, compatibility fields. |
| `animation` | Its `.gltf` under `library/animations/<key>/` | `id`, `file`, standalone/source metadata already present. |
| `material` | Its directory under `library/materials/<key>/` | `name`, map roles/channels/inversion, `flipNormalGreen`; map `file` values are filenames within that directory. |
| `audio` | Its media file under `library/audio/<key>/` | `id`, logical `file` declaration and existing metadata. |

For 0.4.1, the parser accepts a 16- or 64-character hexadecimal set key, a single
leaf filename for scene/audio paths, and a directory path for materials. Paths
use forward slashes and are relative to the library root. They cannot contain
traversal components or absolute paths. Let the helper write these fields.

In the **derived cache**, media locations are resolved into `relPath`,
`fileset.gltf`, `fileset.bin`, `fileset.textures`, `material.maps[].file`, and
`audio.file`. Material map paths there are full library-relative paths, unlike
the source manifest's map filenames. Walk `variants` recursively in both documents.

The migrator rewrites glTF buffer/image URIs, preserves scene data, and deduplicates
shared model textures by SHA-256 content. Preserve its animation identity markers:
`extras.toyboxSourcePath` or `extensions.TOYBOX_library.sourcePath`, and existing
`extras.toyboxAnimation` data. Removing them can change later clip identities.

## 7. Validate before reporting success

Perform read-only checks against the snapshots and the resulting files:

- The command exited successfully, `storageVersion` is `1`, the durable journal
  is at version `3` for this release, and both pending markers are absent.
  A version marker alone is insufficient: publication can precede cleanup.
- Source IDs, cached IDs, ordering, and variant membership match their respective
  baselines. No item was silently dropped, duplicated, renamed, or regrouped.
- Every registered primary/variant file set uses the correct type directory.
  Cache references match their relocated source sets. Every referenced file exists,
  including model/animation buffers and all texture maps. A legacy cached clip
  may still reference a registered model's source file until a separately
  authorized extraction step; preserve that relationship rather than extracting
  animations just to force every cached clip path into `animations/`.
- Each external glTF URI resolves to its intended file inside the library.
  Verify resolved paths, not texture basenames. Two files called `Atlas.png`
  may contain different data.
- Favorites, custom tags, collections, preview-model choices, rig bindings and
  matrices, animation facts, audio facts, material channel assignments, provenance,
  and unrelated fields are preserved. Compare parsed numeric values without
  rounding away differences or reserializing through low-precision floats.
- For a library already at migration version `2`, cached non-path data should
  be unchanged. Exclude only the known media-location fields from a comparison;
  do not drop whole `user`, `rigs`, `material`, `audio`, or `clip` objects.
- For an older library, additionally account for steps `1` and `2`: explicit
  kinds and directly corresponding provenance/tags may be added. Preserve custom
  tags and explicit values. Review these additions rather than ignoring all
  metadata differences.
- The reported backup directory exists. Known old source trees' unindexed extras
  and pack metadata are archived there. Do not delete unknown directories to
  make the filesystem look tidy; they may contain unrelated data.

The migration verifies copied media hashes itself. Do not demand byte-identical
glTF documents: their resource URIs and identity markers legitimately change.
For subsequent import work, use the plugin commit path so the host applies the
same storage rules. Do not manually add a new `library/<source>/<category>` tree.

## 8. Handle failure and recovery

Keep the root, cache location, journals, originals, destinations, and backups
together. After addressing the cause, rerun the same migration command. Never
delete a pending marker, edit an applied version number, or manufacture a
`storagePath` merely to make a failed run appear complete.

| Failure | Action |
| --- | --- |
| Another process holds the storage lock | Close the relevant Toybox instance/watcher and retry. The existence of `storage.lock` alone is normal; do not remove it to bypass a live OS lock. |
| Windows blocks archiving an old directory | Close or navigate the specific Explorer/folder window away, then retry. Indexed media and catalogs may already have moved. |
| Missing source file, unsupported multi-buffer scene, invalid/external URI, or path escape | Inspect the exact entry and restore/repair it only from known source data within the authorized scope. Do not invent replacement textures or silently drop the asset. |
| Insufficient free space | Keep originals and partially copied files. Make space elsewhere, then retry; verified matching destinations can be reused. |
| Conflicting explicit provenance | Show both values and obtain the owner's choice if no existing instruction resolves them. Do not guess a winner. |
| Catalog, manifest, or journal edited externally | Preserve both versions and the transaction. Stop automatic recovery; reconcile the actual edits deliberately. |
| A verified destination is missing or changed | Preserve remaining originals and backups. Investigate the transaction's expected hashes; do not delete originals or rewrite expected hashes to suppress the error. |
| Library has a newer durable version | Use a compatible newer Toybox build. Do not lower its journal version or run an older reader against it. |

Metadata migrations use `_library_config/migration-pending.json` and can restore
their before snapshots before retrying. The storage migration uses
`_library_config/storage-pending.json` to resume publication/cleanup toward the
new layout. These are different recovery protocols; let the runner choose.

Storage backups contain previous manifest/cache documents when applicable,
original scene JSON, a `transaction.json` with source/target hashes and locations,
and `unindexed/` source extras. This is not a second complete media library:
unchanged binary/image/audio data is generally at its managed destination.
Restoring an old manifest alone would point at removed paths. There is no
built-in rollback command; a requested rollback needs a complete backup or a
careful, separately validated reverse restoration using the transaction and
original scene documents. Do not perform that as routine cleanup.

## 9. Report a concrete result

Report the actual library root, tool/version used, counts by kind, whether the
matching cache was migrated/validated, preservation-check results, backup
location, and any remaining blocker. State whether a watcher or app was stopped
and left stopped. Do not claim completion while a pending marker or unresolved
file/reference error remains. Keep enrichment and reindexing as separate work.

## Implementation references

- [Storage overview](library-storage.md)
- [Migration CLI](../src-tauri/src/bin/migrate-library.rs)
- [CLI orchestration and report](../src-tauri/src/maintenance.rs)
- [Ordered migrations and metadata recovery](../src-tauri/src/infra/migrations.rs)
- [Migration steps and corresponding provenance](../src-tauri/src/domain/library_migrations.rs)
- [Media relocation and storage recovery](../src-tauri/src/infra/storage.rs)
- [Source manifest parser](../src-tauri/src/domain/seed.rs)
- [Derived catalog types](../src-tauri/src/domain/catalog_model.rs)
