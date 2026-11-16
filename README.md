# Toybox

Local asset library for models, materials, animations, and audio. Built with Tauri, React, and TypeScript.

## Library migrations

For an AI-facing procedure, including path discovery, offline commands, validation,
and recovery, read [Migrating an existing library](docs/ai-library-migration.md).
The [storage overview](docs/library-storage.md) describes the managed layout.

Toybox runs ordered migrations before loading or rebuilding a library catalog. The durable migration version is separate from the rebuildable catalog schema version. Each library records completed steps in `_library_config/migrations.json`.

The first two steps make legacy model kinds explicit and move existing pack/source metadata into provenance and searchable tags. Pack `name` maps to source; explicit `publisher`, `url`, and `license` fields map directly. If no title exists, the pack identifier is copied verbatim. Existing provenance takes precedence, and matching cached metadata is transferred by asset ID. There is no filename analysis, publisher/license guessing, web lookup, or metadata enrichment. Asset IDs, custom tags, favorites, collections, and unrelated JSON fields are preserved.

Step 3, introduced in 0.4.1, moves indexed media into uniform storage by type and writes explicit file locations. Pack/category names no longer determine storage. Copies are SHA-256 verified before old indexed files are removed; glTF URIs and cached paths are updated together. Existing item identities and user metadata remain attached to the same items. Unindexed source extras are archived with migration backups.

Changed documents have snapshots under `_library_config/migration-backups/`. Metadata transactions can restore their before snapshots before retrying; the separate storage journal resumes publication and cleanup toward the new layout. Recovery refuses to overwrite subsequent external edits. Conflicting explicit provenance in the manifest and cached user metadata stops migration for a deliberate resolution. A library with a newer migration version is rejected rather than downgraded. Only a cache belonging to the same library is migrated.

To add an upgrade, append a numbered step in `src-tauri/src/domain/library_migrations.rs`, increase `LATEST_VERSION`, and keep the transform deterministic and idempotent. Add any explicitly corresponding inputs to the infrastructure runner. Never reuse or reorder an existing version. Derived catalog changes still require a `SCHEMA_VERSION` bump.

## Application updates and releases

Installed release builds check for updates automatically and offer **Install and restart** in a toast and **Settings → Updates**. Downloads are signature-verified by Tauri. Thumbnail writers drain before installation. Development builds do not self-update.

The updater uses GitHub Releases for `dustinlacewell/toybox`, including its signed `latest.json`. Release tags, `package.json`, Cargo, and Tauri configuration must have the same version. The release workflow validates this, signs update artifacts, and publishes the updater manifest. See [Tauri updater documentation](https://v2.tauri.app/plugin/updater/).

One-time release setup:

1. Preserve the updater private key outside the repository. This workstation's key is `%USERPROFILE%\.tauri\toybox-updater.key`; its matching public key is embedded in `tauri.conf.json`.
2. Run `node scripts/configure-update-signing.mjs` with GitHub CLI authenticated as a repository administrator. It checks the public key against the app configuration and uploads the exact private-key contents as the `TAURI_SIGNING_PRIVATE_KEY` Actions secret, without shell newline conversion. An optional argument selects another key path. Set `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` separately if the key is password-protected. Never commit the private key.
3. Push a matching version tag to start the release workflow. It builds all desktop targets into a draft release. Verify the installers, signatures, and `latest.json`, then publish the draft; the website deploys after publication. The first updater-enabled build is `0.4.0`. Users of older builds need that upgrade once; subsequent releases can install in-app.

For a local signed build, set `TAURI_SIGNING_PRIVATE_KEY` to the private-key path in the build process environment before `pnpm tauri build`. Signing credentials are not loaded from `.env` files.

Bundled plugins update with the app. Replaced copies are retained under app-data `plugin-backups/`. File receipts detect and preserve edits to managed installs; linked development plugins remain untouched. Legacy installations with matching bundled plugin IDs are backed up before adoption.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
