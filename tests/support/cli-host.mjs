// A stand-in for the host that `tbx` provides to a plugin's command-line module:
// real temp folders behind the same jailed file interface as
// src-tauri/src/commands/plugins.rs (reads under a caller-supplied source root,
// writes under an authorized root, nothing may escape its root), the extension
// table of the loaded modules, and `commit` that records what would be committed.

import { createHash } from "node:crypto";
import { cpSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const repo = path.resolve(import.meta.dirname, "..", "..");
export const pluginDir = (id) => path.join(repo, "plugins", id);
export const manifestOf = (id) => JSON.parse(readFileSync(path.join(pluginDir(id), "manifest.json"), "utf8"));
export const pluginIds = () => readdirSync(path.join(repo, "plugins")).filter((name) => existsSync(path.join(repo, "plugins", name, "manifest.json"))).sort();

const made = [];
process.on("exit", () => { for (const dir of made) { try { rmSync(dir, { recursive: true, force: true }); } catch { /* best effort */ } } });

/** A folder removed when the process ends. */
export function tempDir(label = "tbx") {
  const dir = mkdtempSync(path.join(tmpdir(), `toybox-${label}-`));
  made.push(dir);
  return dir;
}

/** Write `files` ({ "a/b.txt": string | Uint8Array }) under `root`. */
export function writeTree(root, files) {
  for (const [rel, content] of Object.entries(files)) {
    const target = path.join(root, rel);
    mkdirSync(path.dirname(target), { recursive: true });
    writeFileSync(target, content);
  }
  return root;
}

/** Every file under `root` as forward-slashed relative paths, sorted. */
export function listTree(root) {
  const out = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full); else out.push(path.relative(root, full).split(path.sep).join("/"));
    }
  };
  if (existsSync(root)) walk(root);
  return out.sort();
}

/** The jail of the real host: a path (relative, or absolute under the root) may not leave `root`. */
export function jail(root, p) {
  const base = path.resolve(root);
  const abs = path.resolve(path.isAbsolute(p) ? p : path.join(base, p));
  const rel = path.relative(base, abs);
  if (rel.startsWith("..") || path.isAbsolute(rel)) throw new Error(`path escapes its jail: ${p}`);
  return abs;
}

export function createFs() {
  const refuse = (what) => async () => { throw new Error(`${what} is not available on the command line`); };
  return {
    async readDir(root, p) {
      const abs = jail(root, p);
      return readdirSync(abs, { withFileTypes: true }).map((entry) => ({
        name: entry.name,
        relPath: path.relative(path.resolve(root), path.join(abs, entry.name)).split(path.sep).join("/"),
        isDir: entry.isDirectory(),
      }));
    },
    async readBytes(root, p) { return new Uint8Array(readFileSync(jail(root, p))); },
    async hashFile(root, p) { return createHash("sha256").update(readFileSync(jail(root, p))).digest("hex"); },
    async fileSize(root, p) { return statSync(jail(root, p)).size; },
    async readImageThumb() { throw new Error("no image support in tests"); },
    async copyFile(sourceRoot, p, authorizedRoot, target) {
      const to = jail(authorizedRoot, target);
      mkdirSync(path.dirname(to), { recursive: true });
      copyFileSync(jail(sourceRoot, p), to);
    },
    async writeBytes(root, p, bytes) { const to = jail(root, p); mkdirSync(path.dirname(to), { recursive: true }); writeFileSync(to, bytes); },
    async writeText(root, p, text) { const to = jail(root, p); mkdirSync(path.dirname(to), { recursive: true }); writeFileSync(to, text); },
    async writeJson(root, p, value) { const to = jail(root, p); mkdirSync(path.dirname(to), { recursive: true }); writeFileSync(to, JSON.stringify(value)); },
    async removeTree(root, p) {
      const target = jail(root, p);
      if (target === path.resolve(root)) throw new Error("the root of a jail cannot be removed");
      rmSync(target, { recursive: true, force: true });
    },
    pickDirectory: refuse("pickDirectory"),
    pickSaveFile: refuse("pickSaveFile"),
  };
}

const camel = (name) => name.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());

/** What the host does with a spec before a plugin sees it: defaults applied, flags false, keys camelCased. */
export function withDefaults(specs = [], given = {}) {
  const values = {};
  for (const spec of specs) {
    const key = camel(spec.name);
    if (given[key] !== undefined) values[key] = given[key];
    else if (spec.default !== undefined) values[key] = spec.default;
    else if (spec.type === "flag") values[key] = false;
  }
  return values;
}

/**
 * Load the `cli.module` of each plugin in `ids`, as `tbx` would: only that file,
 * with a context whose extension lookup serves every loaded module.
 * `env`: { libraryRoot, services? }.
 */
export async function loadPlugins(ids, env) {
  const fs = createFs();
  const table = new Map(); // point -> [{ pluginId, value }]
  const lookup = (point) => (table.get(point) ?? []).slice().sort((a, b) => (a.pluginId < b.pluginId ? -1 : a.pluginId > b.pluginId ? 1 : 0));
  const host = {
    getLibraryRoot: async () => env.libraryRoot,
    getSelectedAssets: () => [], getAsset: () => undefined, getSelectedAssetsAsync: async () => [], getAssetAsync: async (id) => library.get(id),
    getDerivedAssets: async (id) => [...library.values()].filter((asset) => asset.origin?.derivedFrom === id),
    openAsset: () => {},
    /** The pretend library is local: its files are always there. */
    ensureAssetFiles: async () => {},
    convertToGltf: async () => { throw new Error("no converter"); },
    pickDirectory: fs.pickDirectory, pickSaveFile: fs.pickSaveFile,
  };
  const services = env.services ?? defaultServices();
  /** The items of the pretend library, by id, when `env.store` is set (see `commit`). */
  const library = new Map();
  const plugins = new Map();
  for (const id of ids) {
    const manifest = manifestOf(id);
    if (!manifest.cli) throw new Error(`${id} has no cli section`);
    const file = path.join(pluginDir(id), manifest.cli.module);
    const imported = await import(pathToFileURL(file).href);
    const def = imported.default;
    const mod = typeof def === "function" ? await def({ id, host, fs, services, extensions: lookup }) : def;
    for (const [point, values] of Object.entries(mod?.extensions ?? {})) {
      table.set(point, [...(table.get(point) ?? []), ...values.map((value) => ({ pluginId: id, value }))]);
    }
    plugins.set(id, { id, manifest, module: mod });
  }
  const committed = [];
  const logs = [];
  const warnings = [];
  const printed = [];
  const base = (pluginId, extra = {}) => ({
    library: { id: "lib-1", name: "Test library" },
    host, fs, services, extensions: lookup,
    commit: async (entries) => {
      committed.push(...entries);
      if (env.store) for (const entry of entries) storeEntry(env.libraryRoot, library, pluginId, entry);
    },
    log: (message) => logs.push(message),
    warn: (message) => warnings.push(message),
    signal: new AbortController().signal,
    ...extra,
  });
  return {
    plugins, committed, logs, warnings, printed, fs, lookup, library, host,
    /** `run.commit` of a run of `pluginId`. */
    commit: (pluginId, entries) => base(pluginId).commit(entries),
    /** `tbx import <name> <paths> [--option ...]`. */
    async runImporter(pluginId, name, paths, options = {}) {
      const { manifest, module } = plugins.get(pluginId);
      const spec = manifest.cli.importers.find((item) => item.name === name);
      const handler = module.cli.importers[name];
      return handler(base(pluginId, { paths, options: withDefaults(spec.options, options) }));
    },
    /** `tbx <path words> <args> [--option ...]`. */
    async runCommand(pluginId, commandPath, args = {}, options = {}, ids) {
      const { manifest, module } = plugins.get(pluginId);
      let specs = manifest.cli.commands;
      let spec;
      for (const word of commandPath.split(" ")) { spec = specs.find((item) => item.name === word); specs = spec.commands ?? []; }
      const handler = module.cli.commands[commandPath];
      const result = await handler(base(pluginId, { args: withDefaults(spec.args, args), options: withDefaults(spec.options, options), print: (text) => printed.push(text), ...(ids ? { assets: ids.map((id) => library.get(id)) } : {}) }));
      return result;
    },
  };
}

export function defaultServices(over = {}) {
  const scratch = tempDir("scratch");
  return {
    settings: { get: async () => ({}), set: async () => {} },
    tools: { launch: async () => {}, run: async () => ({ code: 0, stdout: "", stderr: "" }), hostProcessId: async () => 4242 },
    local: { isOpen: async () => false, requestJson: async () => { throw new Error("no local service"); } },
    scratchDir: async () => scratch,
    ...over,
  };
}

/**
 * What the host does with a committed material or plugin-type entry: move the staged folder
 * (`library/<pack>/materials/<name>/`, `library/<pack>/<type>/<name>/`) to managed storage (`library/materials/<key>/`,
 * `library/<type>/<key>/`), replace an item of the same id (its old folder goes, its tags and other user data stay)
 * and list the files as library-relative paths (a material's in `origin.files`, a plugin item's in `files`).
 * `origin.derivedFrom` and `format` are kept as given.
 */
function storeEntry(libraryRoot, library, pluginId, entry) {
  const key = entry.id.replace(/[^a-z0-9]/gi, "");
  const folder = entry.kind === "plugin" ? entry.type : "materials";
  const staged = path.join(libraryRoot, "library", entry.pack, folder, entry.name);
  const final = path.join(libraryRoot, "library", folder, key);
  const before = library.get(entry.id);
  if (before) rmSync(path.join(libraryRoot, path.dirname(before.relPath)), { recursive: true, force: true });
  mkdirSync(path.dirname(final), { recursive: true });
  cpSync(staged, final, { recursive: true });
  rmSync(staged, { recursive: true, force: true });
  const stored = (file) => `library/${folder}/${key}/${file}`;
  const derivedFrom = entry.origin?.derivedFrom ? { derivedFrom: entry.origin.derivedFrom } : {};
  const common = {
    id: entry.id, library: "lib-1", name: entry.name, category: entry.category, pack: "imported",
    ...(entry.format ? { format: entry.format } : {}),
    user: before?.user ?? { favorite: false, tags: [], provenance: entry.provenance },
  };
  if (entry.kind === "plugin") {
    library.set(entry.id, {
      ...common, kind: "plugin", type: entry.type, fileName: entry.files[0], relPath: stored(entry.files[0]),
      files: entry.files.map(stored), meta: entry.meta ?? {},
      origin: { plugin: pluginId, ...(entry.origin?.meta ? { meta: entry.origin.meta } : {}), ...derivedFrom },
    });
    return;
  }
  library.set(entry.id, {
    ...common, kind: "material", fileName: entry.maps[0].file, relPath: stored(entry.maps[0].file),
    origin: { plugin: pluginId, files: (entry.origin?.files ?? []).map(stored), meta: entry.origin?.meta, ...derivedFrom },
    material: { maps: entry.maps, flipNormalGreen: false },
  });
}
