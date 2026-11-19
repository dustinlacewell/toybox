# The command line (`tbx`)

`tbx` does from a terminal what the app does in its window: manage libraries,
find items, edit their tags and details, import, export, copy between
libraries, delete, and manage plugins. Plugins add importers, exporters and
commands of their own. It is one self-contained program and needs neither the
app to be running nor Node.

`tbx` and the app are two front ends of the same code:

| Layer | Crate | Used by |
| --- | --- | --- |
| Library logic | `toybox-core`, `toybox-remote` | app, server, `tbx` |
| Host: registered libraries, plugins, settings, routing a request to a local or remote library | `toybox-host` | app, `tbx` |
| Plugin runtime outside a window (an embedded JavaScript engine) | `toybox-script` | `tbx` |
| The commands | `toybox-cli` (binary `tbx`) | |

A plugin has one implementation of its work, in modules with no user
interface. Its panel in the app and its `tbx` commands both call them.

## Installing

`tbx` comes with the desktop app and is replaced together with it by the app's updater, so it is always the app's
version (`tbx --version` is the version of the app and of `toybox-server`). It is installed next to the app's own
program:

| Platform | Where `tbx` is | On the PATH |
| --- | --- | --- |
| Windows (installer) | `<install folder>\tbx.exe` | The installer runs `tbx self install`: a copy of `tbx.exe` goes into `<install folder>\bin\`, a folder with nothing else in it, and that folder is added to **your** PATH (no administrator; an update does not add it twice; uninstalling removes it). The install folder itself is not put on PATH because it also holds `toybox.exe` and `uninstall.exe`. Open a new terminal afterwards. |
| macOS | `Toybox.app/Contents/MacOS/tbx` | Not by itself: an app cannot write `/usr/local/bin` silently. Settings > Updates > Command line has "Add to my PATH", which makes the link `~/.local/bin/tbx` (and says if that folder is not on your PATH). The link stays valid across app updates. |
| Linux `.deb` / `.rpm` | `/usr/bin/tbx` | Yes, the package puts it there. |
| Linux AppImage | inside the AppImage | "Add to my PATH" copies it to `~/.local/bin/tbx` (the AppImage's own path changes on every run, so a link would break); the row shows when the copy is older than the app and offers to update it. |

The same from a terminal, for your user only: `tbx self install`, `tbx self uninstall`, `tbx self status`
(works without any libraries). In Settings the row shows whether a terminal finds `tbx`, where the bundled one is, and
a button to add it, update it or remove it. `tbx status` prints the version and path of the running `tbx` and warns
when it differs from the installed app (the app leaves its version in the data folder each time it starts):
"tbx 0.4.0 is older than the installed Toybox 0.4.1. Run the one that came with it ...".

**Without the app** (a server, a build machine): every release has `tbx-<target>[.exe]` and a `.sig` file for Windows,
macOS (Apple Silicon and Intel) and Linux (x86-64 and ARM64), signed with the key that signs the app's updates
(`minisign -V -p <key> -m tbx-<target>`; the key is `plugins.updater.pubkey` in `tauri.conf.json`, base64 of the
minisign key file). Download, make it executable, run `tbx self install`. Plugins are installed with
`tbx plugin install <DIR>`. Such a copy does not update itself: download the new one with the new release.

## Building

```
node_modules/.bin/tsc -p packages/toybox-sdk/tsconfig.json
cd src-tauri
cargo build --release -p toybox-cli
```

The program is `src-tauri/target/release/tbx` (`tbx.exe` on Windows). The
plugin SDK is compiled into it, which is why the SDK is built first; the build
stops with that instruction when `packages/toybox-sdk/dist` is missing
(`TOYBOX_SDK_DIST` names another folder). `tbx` is not part of the default
workspace build. Release installers include it (see Installing): the release workflow builds it with
`node scripts/prepare-tbx-sidecar.mjs`, which puts it in `src-tauri/binaries/` for Tauri's `bundle.externalBin`,
configured only in `src-tauri/tauri.release.conf.json` so `tauri dev` and a plain `tauri build` work without it.

Plugins are found in the `plugins` folder of the data folder, where the app
installs the bundled ones the first time it runs. On a machine without the app,
install them with `tbx plugin install <DIR>`.

The desktop app's package is `toybox-client`; the program it builds is still
`toybox`.

## Conventions

```
tbx [GLOBAL OPTIONS] <COMMAND> [ARGS]
```

| Global option | Environment | Meaning |
| --- | --- | --- |
| `-L, --library <NAME\|ID>` | `TOYBOX_LIBRARY` | Narrow to one library. Commands that only read (`search`, `count`, `types`, `facets`, `tag list`, `collection list`, `show`, `path`) cover every registered library without it; commands that change something act on this library, else on the default library. A name is matched case-insensitively and must be unambiguous. |
| `--json` | | Print one JSON document on standard output in place of text. |
| `-q, --quiet` | | Print no summaries or progress: only errors, the data a command exists to print (ids, tables, paths) and, under `--json`, the result. |
| `-y, --yes` | | Do not ask before a destructive change. |
| `--home <DIR>` | `TOYBOX_HOME` | Use this folder for everything Toybox keeps per user (settings, plugins, caches, personal data); the cache is `<DIR>/cache`. For tests and automation. |
| `--app-id <ID>` | `TOYBOX_APP_ID` | Use the data of the app with this identifier. Default `com.toybox.app`; a development build uses `com.toybox.dev`. Ignored with `--home`. `tbx` never creates the data folder just to read: on a machine that has never run Toybox, read-only commands print what there is (nothing) and leave no folder behind. |
| `-v, --verbose` | | Show what plugin code writes to its `console` (otherwise hidden). |
| `--color <auto\|always\|never>` | `NO_COLOR` | Colour on standard error (only the `error:`, `warning:` and `hint:` labels are coloured). |

- **Output.** Text output is for people: aligned columns, no decoration when
  standard output is not a terminal. `--json` output is for programs: field
  names are camelCase and are the same as the app's own data; additions are
  allowed, renames are not. Progress and warnings go to standard error in both
  modes, so standard output is always just the result.
- **Exit status.** `0` success. `1` the command failed, or the person said no to
  a question (`cancelled`), or `Ctrl-C` stopped a plugin. `2` wrong usage.
  `3` something named does not exist (a library, an item, a plugin). `4` the
  command finished but some items failed or were skipped (the result says
  which): `library check` found problems, a transfer had failed items, an
  import or export skipped something, a delete could not remove everything.
  A second `Ctrl-C` ends `tbx` at once with status `130`.
- **Quiet.** `-q` removes summaries and progress, not what a command exists to
  print (ids, tables, paths) and not what an import or export skipped.
- **Errors** are one line on standard error, `error: <what happened>`, followed
  where it helps by `hint: <what to do>`. With `--json` the line is
  `{"error":{"code":"not_found","message":"..."}}` on standard error. Codes:
  `failed`, `usage`, `not_found`, `partial`, `cancelled`.
- **Changes ask first** when they destroy something (`delete`, `move`,
  `library remove`, `plugin remove`, `thumbs clear`) and standard input is a
  terminal. Without a terminal they refuse (exit `1`) unless `--yes` is given.
  Every command that changes items accepts `--dry-run`: it prints what would
  change and changes nothing.
- **Secrets** are never arguments. An access token is read from
  `TOYBOX_TOKEN`, from standard input with `--token-stdin`, or typed at a
  prompt that does not echo. It is stored in the operating system's credential
  store, as the app does. (`TOYBOX_SECRETS=memory:<file>` keeps tokens in that
  plain file instead, for tests and automation; the file is not protected.)
- **The app may be running.** Both write through the same database and take
  the same short lock per change, so they do not corrupt each other; a change
  made here shows in the app when it next reloads its list. Changes to the set
  of registered libraries or to plugin switches are read by the app at start.

### Selecting items

Commands that act on items take a *selection*: ids, a filter, or both.

```
[ID]...                      item ids as `tbx search` prints them; `-` reads ids from standard input, one per line
--type <TYPE>                model, material, animation, audio, skybox, ... (required with a filter)
--text <TEXT>                part of the name or of a tag
--tag <TAG>                  repeatable; every one must be present
--source, --publisher, --license, --format, --category <VALUE>   repeatable; any one of a kind matches
--collection <NAME>
--favorites
```

With ids, the filter options narrow the ids. With a filter and no ids, every
match is selected; a change through a filter that selects more than 100 items
asks first (or needs `--yes`). Items the app hides because their plugin is off
are not selected by a filter. An id that is not there exits `3` and is named.

A selection that changes something lives in one library (`-L`, else the default
library); an id that is not in it but is in another library gets
`hint: it is in library '<name>'; add -L <name>`. `show` and `path`, which only
read, look ids up in every library when `-L` is not given: an id found in
one library is used, an id found in several is shown for each, labelled with
its library. In `edit`, `--source`, `--publisher` and `--license` set values,
so they are not selection filters there.

## Commands

### Overview

| Command | Does |
| --- | --- |
| `tbx status` | Where the data is, the libraries with their item counts, the plugins that are on, this `tbx`'s version and path, and anything wrong (a library that is missing, a blocked plugin, a remote that is offline or whose server and this `tbx` do not fit, a `tbx` that differs from the installed app). Server versions that merely differ are listed as notices, not problems. |
| `tbx self install\|uninstall\|status` | Put this `tbx` on your PATH for your user, take it off, or say which `tbx` a terminal finds (see Installing). Needs no libraries. |
| `tbx help [COMMAND]...` | Help. `tbx help --all` prints the whole command tree, including plugin commands; with `--json` as data. |
| `tbx completions <bash\|zsh\|fish\|powershell>` | A completion script. |

### Libraries

| Command | Does |
| --- | --- |
| `tbx library list` | Registered libraries: id, name, kind, location, default, state. |
| `tbx library info [LIBRARY]` | One library: counts by type, revision, size of its files, how many kept source files it stores once and how many bytes that sharing saves (and what converting the library to the shared layout freed), for a remote the server, role and cache size. |
| `tbx library create <DIR> [--name NAME] [--default]` | Make a new library folder and register it. |
| `tbx library add <DIR> [--name NAME] [--default]` | Register an existing library folder. |
| `tbx library connect <URL> [--name NAME] [--token-stdin]` | Register a library served by `toybox-server`. |
| `tbx library login <LIBRARY> [--token-stdin]` | Replace the access token of a remote library. |
| `tbx library rename <LIBRARY> <NAME>` | |
| `tbx library default <LIBRARY>` | |
| `tbx library remove <LIBRARY>` | Forget a library. Its files stay where they are (for a remote library its token and downloaded files go; the server is untouched). `--dry-run` prints what would happen. |
| `tbx library scan [LIBRARY]` | Local: rebuild the index from the manifest and the files. Remote: fetch what changed on the server. |
| `tbx library check [LIBRARY] [--fix]` | Verify the database and that every item's files exist, and that each kept source file (stored by content) still is what its name says. Exit `4` when something is wrong. Stored source files no item uses are reported (not an error); `--fix` removes them and changes nothing else. A local library with library-format migrations this version would run on opening gets a line saying so (`pendingMigrations` in JSON; the details are `toybox-server migrate --check`). `tbx library info` shows the same, and for a remote library the server's version, the oldest Toybox it serves and the version sentence (`remote.serverVersion`, `minClientVersion`, `compat`, `compatMessage`). |
| `tbx library cache size\|clear [LIBRARY]` | The downloaded files of a remote library. |
| `tbx library token list [LIBRARY]` | Access tokens of a remote library (needs an admin token). |
| `tbx library token create --name NAME --role read\|write\|admin [LIBRARY]` | Make a token; its secret is printed once, alone, on standard output. |
| `tbx library token revoke <TOKEN_ID> [LIBRARY]` | Revoke a token at once (the id is the one `token list` prints). |

### Finding and reading

| Command | Does |
| --- | --- |
| `tbx search [TEXT] [FILTER] [--sort name] [--desc] [--limit N] [--offset N] [--include-hidden] [--ids]` | List matching items, over every library (the same item in several is listed once, from the best-ranked library: the default first). Default limit 50. `--ids` prints only ids, one per line, for piping. Without `--type` every type is searched (the query runs per type and the pages are merged by name). Items whose plugin is off are hidden, as in the app, unless `--include-hidden`. Columns: [library, when more than one library is registered,] id, type, name, category, source, tags. When nothing matches, `No items match.` is printed on standard error. |
| `tbx count [TEXT] [FILTER] [--include-hidden]` | How many match. |
| `tbx show <ID>...` | Everything about items: names, type, tags, details, files with sizes, the source files kept with each (listed apart: other items may keep the same one), variants, where they came from. Takes a selection. |
| `tbx path <ID>... [--all]` | The absolute path of each item's main file (`--all`: every file). Files of a remote library are downloaded first. Takes a selection. |
| `tbx types` | The asset types with their counts and the plugin that declares each; `off` marks a type whose plugin is off. |
| `tbx facets --type <TYPE> [--tags N]` | The values that can be filtered on, with counts: sources, publishers, licenses, formats, categories, tags. Default 20 tags. |

`--all-libraries` is still accepted by `search`, `count`, `types` and `facets` and does
nothing: they already cover every library unless `-L` is given.

### Tags, details, favourites, collections

Library tags and details belong to the library and are seen by everyone who
uses it. Favourites and collections are personal to this computer.

| Command | Does |
| --- | --- |
| `tbx tag list [--type TYPE] [--search TEXT] [--limit N]` | Tags in use, with counts (default 50), over every library; without `--type`, summed over every type. |
| `tbx tag add <TAGS> <SELECTION>` | Add library tags (comma-separated). |
| `tbx tag remove <TAGS> <SELECTION>` | |
| `tbx tag set <TAGS> <SELECTION>` | Replace the tags; an empty string removes them all. |
| `tbx edit <SELECTION> [--source TEXT] [--publisher TEXT] [--license TEXT] [--url URL]` | Change details. An empty value clears the field. |
| `tbx favorite add\|remove <SELECTION>` | |
| `tbx collection list [--type TYPE]` | Collections with their sizes, over every library. |
| `tbx collection add\|remove <NAME> <SELECTION>` | Put items into, or take them out of, a collection. A collection exists while it has items. |

### Bringing in, sending out, moving, deleting

| Command | Does |
| --- | --- |
| `tbx import` | List the importers the active plugins provide. |
| `tbx import <IMPORTER> <PATH>... [IMPORTER OPTIONS] [--tag TAGS] [--source TEXT] [--publisher TEXT] [--license TEXT] [--url URL] [--dry-run]` | Import files or folders into the library. The options after the paths are the importer's own (`tbx import <IMPORTER> --help`). The shared options set tags and details on everything imported (`--tag` takes comma-separated tags). Prints the ids of the items now in the library, as `tbx show` takes them (an animation file is listed by its clips). Everything the importer skipped is printed on standard error as `skipped: <reason>` (and is in `skipped` under `--json`), and the exit status is `4` when anything was skipped. An item that replaced one the library already had (an importer that can tell, such as the Substance one) is said on standard error as `replaced: <name> (was <what it was>)` and listed in `replaced` (ids) and `replacedItems` (`{id, name, was}`) under `--json`. When nothing new came in the summary says so. Thumbnails are drawn by the app the next time it opens the library. |
| `tbx export` | List the exporters. |
| `tbx export copy <SELECTION> --to <DIR> [--preserve-structure] [--dry-run]` | Copy the items' files out. `--dry-run` prints the paths that would be written. |
| `tbx export glb <SELECTION> --to <DIR> [--preserve-structure] [--dry-run]` | Write each model as one self-contained `.glb`. An item that cannot be one (a sound) fails the command. |
| `tbx export <EXPORTER> <SELECTION> --to <DIR> [--preserve-structure] [--variants all\|best] [--preview] [EXPORTER OPTIONS]` | Run a plugin's exporter. `--variants` is passed to the exporter as `variantPolicy` (default `all`); `--preview` only plans and prints the paths. |
| `tbx copy <SELECTION> --to <LIBRARY> [--on-conflict skip\|replace] [--with-clips] [--dry-run]` | Copy items to another library, local or remote. An item already there is skipped (default) or replaced. |
| `tbx move <SELECTION> --to <LIBRARY> [--on-conflict skip\|replace] [--with-clips] [--dry-run]` | Copy, then remove from the source the items that arrived. |
| `tbx delete <SELECTION> [--dry-run]` | Delete items and their files from the library. |
| `tbx thumbs status` | How many items still wait for a thumbnail. |
| `tbx thumbs clear [SELECTION] [--dry-run]` | Discard thumbnails so the app draws them again; with no selection, all of them. |

### Plugins and settings

| Command | Does |
| --- | --- |
| `tbx plugin list` | Installed plugins: id, version, on/off/blocked (with the reason), what each provides. A plugin with nothing of its own to list shows `extends <the plugins it depends on>`. Command-line declarations that are not mounted (a name already taken) are listed as notes. |
| `tbx plugin info <ID>` | One plugin: its manifest in words, dependencies and dependents, settings, and the commands it adds here. |
| `tbx plugin enable\|disable <ID>` | Same rules as the app: enabling turns on what it needs; disabling is refused while something needs it. Prints what else changed. |
| `tbx plugin install <DIR> [--link]` | Install a plugin folder (copy, or link for development) as `<plugins>/<manifest id>`. Refuses an id that is installed. |
| `tbx plugin remove <ID>` | Remove a plugin (a linked plugin's folder is left alone). Refused while a switched-on plugin needs it; its settings are forgotten. |
| `tbx plugin settings <ID> [KEY [VALUE]]` | Show, read or set a plugin's declared settings. `--unset` returns one to its default. |
| `tbx config list\|get <KEY>\|set <KEY> <VALUE>\|unset <KEY>` | App settings that are not a plugin's (`fbx2gltf-path`). |

### Looking inside

| Command | Does |
| --- | --- |
| `tbx debug paths` | Every folder and file `tbx` reads or writes. |
| `tbx debug sql <QUERY> [--limit N]` | Run one read-only SQL query on the database of the library chosen with `-L` (else the default), printing at most `N` rows (default 1000, `0`: all). The schema is in [SQLite storage](sqlite-storage.md) and is not a stable interface. |

## Plugins and the command line

A plugin adds to `tbx` in three ways. Each is declared in the manifest, so help,
validation and completion work without running any plugin code, and implemented
in the plugin's command-line module.

```json
{
  "cli": {
    "module": "dist/cli.js",
    "importers": [
      {
        "name": "audio",
        "summary": "Import WAV, MP3, Ogg Vorbis and FLAC files.",
        "options": [
          { "name": "category", "type": "string", "default": "sounds", "help": "Category of the imported sounds." }
        ]
      }
    ],
    "exporter": { "name": "placer", "summary": "Publish models into a Godot asset_placer library.", "options": [] },
    "commands": [
      {
        "name": "sbsar",
        "summary": "Work with Substance archives.",
        "commands": [
          {
            "name": "inputs",
            "summary": "List the parameters of an archive.",
            "args": [{ "name": "file", "type": "path", "required": true, "help": "The .sbsar file." }]
          }
        ]
      }
    ]
  }
}
```

- `module` is a plugin-relative ES module with **no user interface**: it must
  not import React, three.js or anything that needs a window. A plugin whose
  `entry` module is already like that names the same file in both. `tbx` loads
  the `cli.module` of every active plugin and never loads `entry` or the
  panels.
- `importers[].name`, `exporter.name` and top-level `commands[].name` are
  lower-case words (`[a-z][a-z0-9-]*`). An importer becomes
  `tbx import <name>`, the exporter `tbx export <name>`, a command
  `tbx <name>`. A name that a built-in command or an earlier plugin (by id)
  already uses is not mounted; `tbx plugin list` says so.
- A command either has `commands` (it is a group) or `args`/`options` (it
  runs). Groups nest.
- An exporter with no `options` takes them from the manifest's `fields`
  (`text` → string, `checkbox` → flag, `select` → choice, `directory` and
  `saveFile` → path).
- A command with `"selection": true` accepts the standard selection and
  receives the selected items. Add `"files": true` when it reads their files or
  the source files kept with them: for a remote library those are downloaded
  before the command runs (the app does the same before it opens an asset
  panel). A command that only reads the items' records leaves it out, so a large
  selection downloads nothing.

**Arguments and options**

| Field | Meaning |
| --- | --- |
| `name` | `kebab-case`. An option is given as `--name`. |
| `type` | `string`, `number`, `flag`, `choice` or `path`. A relative `path` is made absolute against the current folder before the plugin sees it. |
| `choices` | The allowed values of a `choice`. |
| `default` | Used when the option is not given. |
| `required` | The command is refused without it. |
| `multiple` | May be given several times (an argument: takes the rest of the line). The plugin receives an array. |
| `help` | One line. |

The plugin receives values keyed by the camelCase of `name`
(`--flip-green` → `flipGreen`). A flag is `true` or `false`.

**The module**

The module default-exports a `PluginModule` (or a function of `PluginCtx`
returning one, see [Plugins](plugins.md)). Besides the members the app uses, it
may carry `cli`:

```ts
import { definePlugin } from "@ldlework/toybox-sdk";
import { importAudio } from "./lib/import.js";

export default definePlugin((ctx) => ({
  cli: {
    importers: {
      audio: (run) => importAudio(run, run.paths, { category: String(run.options.category) }),
    },
    commands: {
      "sbsar inputs": async (run) => { run.print("..."); return { inputs: [] }; },
    },
  },
}));
```

- `extensions`, as in the app, are collected from every loaded module and
  served by `ctx.extensions(point)`.
- `run` and `preview` are the exporter, exactly as in the app. `tbx export
  <name>` calls them with `config` built from the options, `targetDir` from
  `--to` and `preserveStructure` from `--preserve-structure`.
- `cli.importers[name]` receives an `ImportRun`: `paths` (absolute files and
  folders), `options`, `library` (`{ id, name }`), `host`, `fs`, `services`,
  `extensions`, `commit(entries)`, `log(message)`, `warn(message)` and
  `signal`. It stages files and commits entries exactly as an import panel
  does. Tags and details given with the shared options are applied by the host
  to every committed entry that does not set its own.
- `cli.commands[path]`, keyed by the command's names joined with spaces,
  receives a `CommandRun`: `args`, `options`, `library`, `assets` (when the
  command takes a selection), `host`, `fs`, `services`, `extensions`,
  `commit(entries)`, `print(text)`, `log(message)`, `warn(message)` and
  `signal`. What it returns is the command's result: printed as JSON under
  `--json`, and otherwise printed only when the command printed nothing itself.
  Throwing fails the command with the error's message; when the message ends in a
  line `hint: <what to do>`, that line is shown as the hint (`hint: ...` under the
  `error:` line) instead of as part of the message. A result that is an object with
  a non-empty `skipped` list of strings is a command that left items out: each is
  printed on standard error as `skipped: <reason>` and the exit status is `4`, as
  for an importer.

`host`, `fs` and `services` are the same interfaces as in the app, with the
same permissions. Dialogs do not exist here: `pickDirectory` and `pickSaveFile`
reject. `host.getSelectedAssets()` answers with the selection of the command. One
thing exists on the command line only: `fs.removeTree(authorizedRoot, path)`
removes a folder below a root the run may write to (to tidy up scratch files), so
code that also runs in the app calls it as `fs.removeTree?.(...)`.

The engine provides the JavaScript language and `console`, `TextEncoder`,
`TextDecoder`, `setTimeout`, `clearTimeout`, `AbortController`, `structuredClone`
and `performance.now`. There is no DOM, no `fetch`, no WebGL and no Node API.

## JSON output

With `--json` a command prints exactly one document on standard output (and nothing
else there). Field names are camelCase and, for items, are the app's own: an `asset` is
the item as the app has it (`id`, `name`, `kind`, `category`, `tags`, `user`, `origin`,
...). A field with nothing to say is left out rather than `null`. Errors are
`{"error":{"code","message"}}` on standard error (see Conventions). A change made with
`--dry-run` prints the same document as the real one with `"dryRun": true`.

| Command | Document |
| --- | --- |
| `status` | `{paths:{appData, cache, settings, plugins, exists}, libraries:[Library], plugins:{on[], off[], blocked[]}, problems[]}` |
| `help --all` | `{commands:[{command, summary, plugin?}]}` (`command` as typed after `tbx`) |
| `library list` | `{libraries:[Library]}` where a *Library* is `{id, name, kind: local\|remote, location, default, state: ok\|missing\|read-only\|offline\|unreachable, canWrite, items?}` |
| `library info` | a *Library* plus `{items, revision, types:[{id, count}], files?, bytes?, sources?:{files, bytes, savedBytes, migration?:{items, bytesFreed, skipped}}, remote?:{url, serverName, role, offline, cacheBytes}}` (`files`, `bytes` and `sources` for a local library; `sources` counts the kept source files stored once, with the bytes sharing saves) |
| `library create\|add\|connect\|login\|rename\|default` | `{library: Library}` |
| `library remove` | `{removed: Library, dryRun}` |
| `library scan` | `{library, items, revision}` |
| `library check` | `{library, ok, items, filesChecked, problems:[{item?, message}], orphanSources[], removedSources[]}`; exit `4` when `ok` is false (an orphan source is not a problem) |
| `library cache size` / `clear` | `{library, bytes}` / `{library, freedBytes}` |
| `library token list` | `{tokens:[{id, name, role, createdAt, lastUsedAt?}]}` |
| `library token create` | `{id, name, role, createdAt, token}` (the secret appears only here) |
| `library token revoke` | `{revoked}` |
| `search` | `{total, exact, offset, items:[{library, asset}]}` (`exact` is always true; `library` is the library id) |
| `count` | `{count}` |
| `show` | `{items:[{library, asset, files:[{path, size?}], sources:[{path, size?}]}]}` (`size` is absent for a file that is not on this computer; `sources` are the kept source files) |
| `path` | `{items:[{library, id, path, files?}]}` (`files` with `--all`) |
| `types` | `{types:[{id, label, plugin?, native, count, hidden}]}` |
| `facets` | `{type, total, categories, sources, publishers, licenses, formats, tags, tagTotal, libraries}`, each list `[{value, count}]` |
| `tag list` | `{tags:[{value, count}]}` |
| `tag add\|remove\|set`, `edit`, `favorite`, `collection add\|remove` | `{library, dryRun, changed[], unchanged[]}` (ids) |
| `collection list` | `{collections:[{name, count}]}` |
| `import` (no importer) | `{importers:[{name, plugin, summary}]}` |
| `import <importer>` | `{library, dryRun, imported[], replaced[], replacedItems[], skipped[], warnings[]}` (`imported`: ids of the items now in the library; exit `4` when `skipped` is not empty) |
| `export` (no exporter) | `{exporters:[{name, plugin?, summary}]}` |
| `export copy\|glb\|<exporter>` | `{library, target, preview, written[], skipped[], warnings[]}`; exit `4` when `skipped` is not empty |
| `copy`, `move` | `{from, to, mode, dryRun, copied, replaced, skipped, failed, removed, items:[{id, name, kind, outcome: copied\|replaced\|skipped\|failed, reason?, sourceRemoved}], wouldTransfer[]}` (`wouldTransfer` only for a dry run) |
| `delete` | `{library, dryRun, deleted[], remaining?}` |
| `thumbs status` | `{library, pending, items}` |
| `thumbs clear` | `{library, dryRun, all, cleared?}` |
| `plugin list` | `{plugins:[Plugin]}` where a *Plugin* is `{id, name, version, state: on\|off\|blocked, blockedBy[], provides[], problems[]}` |
| `plugin info` | a *Plugin* plus `{description?, path, dependencies, requiredBy[], types[], importers[], exporter?, commands[], permissions, settings:[{key, type, label, value, default}]}` |
| `plugin enable\|disable` | `{id, enabled, changed[]}` |
| `plugin install` | `{id, name, version, path, linked}` |
| `plugin remove` | `{id}` |
| `plugin settings <ID>` | `{plugin, settings:[{key, type, label, value, default}]}`; with a key `{plugin, key, value}` |
| `config list` | `{settings:{<key>: value\|null}}` |
| `config get\|set\|unset` | `{key, value}` |
| `debug paths` | `{appData, cache, settings, plugins, remoteFiles, pluginScratch, browseIndex, secrets, libraries:[{id, name, root, database, userDatabase, appData, thumbnails}]}` |
| `debug sql` | `{columns[], rows[[...]], truncated}` (a blob is a hex string) |
| a plugin command | what its handler returns, as is |

Under `--json` a plugin's own `print` output goes to standard error, so standard
output stays the one document.
