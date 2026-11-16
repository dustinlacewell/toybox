# Toybox

Local asset library for models, materials, animations, and audio. Built with Tauri, React, and TypeScript.

## Library migrations

Toybox runs ordered migrations before loading or rebuilding a library catalog. The durable migration version is separate from the rebuildable catalog schema version. Each library records completed steps in `_library_config/migrations.json`.

The first two steps make legacy model kinds explicit and move existing pack/source metadata into provenance and searchable tags. Pack `name` maps to source; explicit `publisher`, `url`, and `license` fields map directly. If no title exists, the pack identifier is copied verbatim. Existing provenance takes precedence, and matching cached metadata is transferred by asset ID. There is no filename analysis, publisher/license guessing, web lookup, or metadata enrichment. Asset IDs, files, custom tags, favorites, collections, and unrelated JSON fields are preserved.

Changed documents have before/after snapshots under `_library_config/migration-backups/`. A durable transaction marker allows an interrupted migration to roll back before retrying. Recovery refuses to overwrite subsequent external edits. Conflicting explicit provenance in the manifest and cached user metadata stops migration for a deliberate resolution. A library with a newer migration version is rejected rather than downgraded. Only a cache belonging to the same library is migrated.

To add an upgrade, append a numbered step in `src-tauri/src/domain/library_migrations.rs`, increase `LATEST_VERSION`, and keep the transform deterministic and idempotent. Add any explicitly corresponding inputs to the infrastructure runner. Never reuse or reorder an existing version. Derived catalog changes still require a `SCHEMA_VERSION` bump.

## Application updates and releases

Installed release builds check for updates automatically and offer **Install and restart** in a toast and **Settings → Updates**. Downloads are signature-verified by Tauri. Thumbnail writers drain before installation. Development builds do not self-update.

The updater uses GitHub Releases for `dustinlacewell/toybox`, including its signed `latest.json`. Release tags, `package.json`, Cargo, and Tauri configuration must have the same version. The release workflow validates this, signs update artifacts, and publishes the updater manifest. See [Tauri updater documentation](https://v2.tauri.app/plugin/updater/).

One-time release setup:

1. Preserve the updater private key outside the repository. This workstation's key is `%USERPROFILE%\.tauri\toybox-updater.key`; its matching public key is embedded in `tauri.conf.json`.
2. Run `node scripts/configure-update-signing.mjs` with GitHub CLI authenticated as a repository administrator. It checks the public key against the app configuration and uploads the exact private-key contents as the `TAURI_SIGNING_PRIVATE_KEY` Actions secret, without shell newline conversion. An optional argument selects another key path. Set `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` separately if the key is password-protected. Never commit the private key.
3. Publish a matching version tag through the release workflow. The first updater-enabled build is `0.4.0`. Users of older builds need that upgrade once; subsequent releases can install in-app.

For a local signed build, set `TAURI_SIGNING_PRIVATE_KEY` to the private-key path in the build process environment before `pnpm tauri build`. Signing credentials are not loaded from `.env` files.

Bundled plugins update with the app. Replaced copies are retained under app-data `plugin-backups/`. File receipts detect and preserve edits to managed installs; linked development plugins remain untouched. Legacy installations with matching bundled plugin IDs are backed up before adoption.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
