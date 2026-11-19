// The bootstrap of the `tbx` plugin runtime. Evaluated by `toybox-script` as a module with
// `import.meta.host` (Rust functions bound to toybox-host). It rebuilds, for the command line, what the
// app builds for the webview in src/services: pluginCtx.ts, pluginHost.ts, pluginServices.ts, slotHost.ts,
// pluginExtensions.ts, pluginModule.ts, domain/origin.ts, domain/importProvenance.ts, domain/libraryId.ts.

import { createReportBuilder } from "@ldlework/toybox-sdk";

const host = import.meta.host;

const CANCELLED = "Cancelled";
let run = null; // the run in progress: { pluginId, library, assets, selection, writeJail, defaults, committed, controller }

/** Every Rust call goes through here: a cancelled run aborts its signal when the host refuses. */
const call = (name, ...args) => {
  try {
    return host.call(name, ...args);
  } catch (e) {
    if (e && e.message === CANCELLED && run) run.controller.abort(new Error(CANCELLED));
    throw e;
  }
};

// --- library ids (domain/libraryId.ts) ----------------------------------------------------------

const SEP = "::";
const parseQualified = (id) => {
  const at = typeof id === "string" ? id.indexOf(SEP) : -1;
  if (at <= 0) return null;
  const library = id.slice(0, at);
  return /^[A-Za-z0-9_-]+$/.test(library) ? { library, id: id.slice(at + SEP.length) } : null;
};
const qualifyId = (library, id) => (parseQualified(id) ? id : `${library}${SEP}${id}`);
const unqualify = (library, id) => (id.startsWith(`${library}${SEP}`) ? id.slice(library.length + SEP.length) : id);
const unqualifyEntry = (library, entry) => {
  const out = { ...entry, id: unqualify(library, entry.id) };
  if (entry.variants) out.variants = entry.variants.map((variant) => ({ ...variant, id: unqualify(library, variant.id) }));
  return out;
};

// --- origin and provenance (domain/origin.ts, domain/importProvenance.ts) ------------------------

const withOrigin = (entry, pluginId, attributable = []) => {
  if (!pluginId) return entry;
  const meta = entry.origin?.meta;
  const files = entry.origin?.files;
  const named = entry.origin?.plugin;
  const plugin = named && attributable.includes(named) ? named : pluginId;
  const result = { ...entry, origin: { plugin, ...(meta && Object.keys(meta).length ? { meta } : {}), ...(files?.length ? { files } : {}) } };
  if (entry.variants) result.variants = entry.variants.map((variant) => withOrigin(variant, pluginId, attributable));
  return result;
};

const cleanProvenance = (value) => {
  const kept = Object.entries(value ?? {}).flatMap(([key, v]) => (typeof v === "string" && v.trim() ? [[key, v.trim()]] : []));
  return kept.length ? Object.fromEntries(kept) : undefined;
};

/** The shared options of `tbx import` fill in what an entry leaves unset; what the entry sets wins. */
const withDefaults = (entry, defaults) => {
  const result = { ...entry };
  const provenance = cleanProvenance({ ...cleanProvenance(defaults.provenance), ...cleanProvenance(entry.provenance) });
  if (provenance) result.provenance = provenance;
  if (!(entry.tags && entry.tags.length) && defaults.tags.length) result.tags = [...defaults.tags];
  if (entry.variants) result.variants = entry.variants.map((variant) => withDefaults(variant, defaults));
  return result;
};

// --- permissions (services/pluginHost.ts `gate`) --------------------------------------------------

const gate = (pluginId, capability, granted) => (fn) =>
  granted
    ? fn
    : (..._args) => {
        throw new Error(`plugin '${pluginId}' lacks permission: ${capability}`);
      };

const noDialog = (what) => () =>
  Promise.reject(new Error(`${what} is not available on the command line: there are no dialogs here`));

// --- assets ---------------------------------------------------------------------------------------

const remember = (assets) => {
  for (const asset of assets) {
    run.assets.set(asset.id, asset);
    for (const variant of asset.variants ?? []) run.assets.set(variant.id, variant);
  }
};
const fetchAssets = async (ids) => {
  const wanted = [...new Set(ids.map((id) => qualifyId(run.library.id, id)))].filter((id) => !run.assets.has(id));
  if (wanted.length) remember(call("assets.get", run.library.id, wanted));
};
const loaded = (id) => run?.assets.get(qualifyId(run.library.id, id));
const selected = () => (run ? run.selection.map((id) => run.assets.get(id)).filter(Boolean) : []);

// --- the pieces of a plugin context -----------------------------------------------------------------

let table = new Map();
const extensions = (point) => [...(table.get(point) ?? [])];

const buildFs = (pluginId, perms) => {
  const read = gate(pluginId, "fsRead", perms.fsRead);
  const write = gate(pluginId, "fsWrite", perms.fsWrite);
  const jail = () => run?.writeJail ?? null;
  return {
    readDir: read(async (root, path) => call("fs.readDir", root, path)),
    readBytes: read(async (root, path) => call("fs.readBytes", root, path)),
    readImageThumb: read(async (root, path, max) => call("fs.readImageThumb", root, path, max)),
    copyFile: read(write(async (root, path, authorized, target) => call("fs.copyFile", root, path, authorized, target, jail()))),
    writeBytes: write(async (root, path, bytes) => call("fs.writeBytes", root, path, bytes, jail())),
    writeText: write(async (root, path, text) => call("fs.writeText", root, path, text, jail())),
    writeJson: write(async (root, path, value) => call("fs.writeText", root, path, JSON.stringify(value, null, 2), jail())),
    pickDirectory: noDialog("pickDirectory"),
    pickSaveFile: noDialog("pickSaveFile"),
  };
};

const buildServices = (pluginId, perms) => {
  const tools = gate(pluginId, "runTools", perms.runTools);
  const local = gate(pluginId, "localNetwork", perms.localNetwork);
  return {
    settings: {
      get: async () => call("svc.settingsGet", pluginId),
      set: async (key, value) => call("svc.settingsSet", pluginId, key, value ?? null),
    },
    tools: {
      launch: tools(async (setting, args) => call("svc.toolLaunch", pluginId, setting, args)),
      run: tools(async (setting, args, options) => call("svc.toolRun", pluginId, setting, args, options?.timeoutMs ?? 120000)),
      hostProcessId: tools(async () => call("svc.hostProcessId")),
    },
    local: {
      isOpen: local(async (port) => call("svc.localOpen", pluginId, port)),
      requestJson: local(async (port, message, timeoutMs = 30000) => call("svc.localJson", pluginId, port, message, timeoutMs)),
    },
    scratchDir: async (clear = false) => call("svc.scratch", pluginId, clear),
  };
};

const readSide = () => ({
  getSelectedAssets: () => selected(),
  getAsset: (id) => loaded(id),
  getSelectedAssetsAsync: async () => {
    await fetchAssets(run.selection);
    return selected();
  },
  getAssetAsync: async (id) => {
    await fetchAssets([id]);
    return loaded(id);
  },
});

const buildSlotHost = (pluginId, perms) => {
  const convert = gate(pluginId, "rustConvert", perms.rustConvert);
  return {
    ...readSide(),
    getLibraryRoot: async () => (run ? call("library.info", run.library.id).root ?? null : null),
    convertToGltf: convert(async (src, pack, category, stem) => {
      if (!run) throw new Error("no library configured");
      return call("library.convert", run.library.id, src, pack, category, stem);
    }),
    pickDirectory: noDialog("pickDirectory"),
    pickSaveFile: noDialog("pickSaveFile"),
  };
};

const buildHostApi = (pluginId, perms) => {
  const exp = gate(pluginId, "rustExport", perms.rustExport);
  return {
    ...readSide(),
    previewExport: async (ids, format, preserve) => call("export.preview", ids.map((id) => qualifyId(run.library.id, id)), format, !!preserve),
    getParsedGltf: async (id) => call("export.gltf", qualifyId(run.library.id, id)),
    getAssetUrls: async () => {
      throw new Error("getAssetUrls is not available on the command line: there are no URLs outside the app window");
    },
    assembleGlb: exp(async (id) => call("export.glb", qualifyId(run.library.id, id))),
    performCopy: exp(async (id, targetDir, stem, preserve) => call("export.copy", qualifyId(run.library.id, id), targetDir, stem, !!preserve, run?.writeJail ?? null)),
    placerMerge: exp(async (json, sub, assets) => call("export.placerMerge", json, sub, assets)),
    transcodeImage: exp(async (fileName, bytes) => call("export.transcode", fileName, bytes)),
  };
};

// --- extensions (services/pluginExtensions.ts) -------------------------------------------------------

const isRecord = (value) => !!value && typeof value === "object" && !Array.isArray(value);

const collectExtensions = (contributions) => {
  const result = new Map();
  const problems = [];
  for (const { pluginId, extensions: contributed } of [...contributions].sort((a, b) => (a.pluginId < b.pluginId ? -1 : a.pluginId > b.pluginId ? 1 : 0))) {
    if (contributed === undefined) continue;
    if (!isRecord(contributed)) {
      problems.push([pluginId, "its extensions must be an object keyed by extension point"]);
      continue;
    }
    for (const [point, values] of Object.entries(contributed)) {
      if (!Array.isArray(values)) {
        problems.push([pluginId, `its contribution to '${point}' must be an array`]);
        continue;
      }
      if (!values.length) continue;
      const list = result.get(point) ?? [];
      for (const value of values) list.push({ pluginId, value });
      result.set(point, list);
    }
  }
  return { table: result, problems };
};

// --- loading (services/pluginModule.ts, pluginLoader.ts) ----------------------------------------------

const plugins = new Map(); // id -> { id, module, ctx, hostApi, requiredBy, failed }
const failures = new Map(); // id -> message

const resolvePluginModule = async (id, exported, ctx) => {
  const value = typeof exported === "function" ? await exported(ctx) : exported;
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`plugin '${id}' must default-export a module object or a function that returns one`);
  }
  if (value.run !== undefined && typeof value.run !== "function") throw new Error(`plugin '${id}': run must be a function`);
  if (value.profiles !== undefined && !Array.isArray(value.profiles)) throw new Error(`plugin '${id}': profiles must be an array`);
  if (value.types !== undefined && (!value.types || typeof value.types !== "object" || Array.isArray(value.types))) {
    throw new Error(`plugin '${id}': types must be an object keyed by type id`);
  }
  if (value.cli !== undefined && !isRecord(value.cli)) throw new Error(`plugin '${id}': cli must be an object`);
  return value;
};

const describe = (e) => (e && typeof e === "object" && "message" in e ? String(e.message) : String(e));

/** `specs`: [{ id, path, permissions, requiredBy }] in dependency order. Returns [[id, message]...]. */
export async function loadPlugins(specs) {
  const problems = [];
  const contributions = [];
  for (const spec of specs) {
    const perms = spec.permissions ?? {};
    const ctx = {
      id: spec.id,
      host: buildSlotHost(spec.id, perms),
      fs: buildFs(spec.id, perms),
      services: buildServices(spec.id, perms),
      extensions,
    };
    try {
      const imported = await import(spec.path);
      const module = await resolvePluginModule(spec.id, imported.default, ctx);
      plugins.set(spec.id, { id: spec.id, module, ctx, hostApi: buildHostApi(spec.id, perms), requiredBy: spec.requiredBy ?? [] });
      if (module.extensions !== undefined) contributions.push({ pluginId: spec.id, extensions: module.extensions });
    } catch (e) {
      failures.set(spec.id, describe(e));
      problems.push([spec.id, describe(e)]);
    }
  }
  const { table: collected, problems: extensionProblems } = collectExtensions(contributions);
  table = collected;
  problems.push(...extensionProblems);
  return problems;
}

// --- runs ---------------------------------------------------------------------------------------------

const needPlugin = (id) => {
  const plugin = plugins.get(id);
  if (!plugin) throw new Error(failures.has(id) ? `plugin '${id}' did not load: ${failures.get(id)}` : `plugin '${id}' is not loaded`);
  return plugin;
};

const applyDefaults = (values, specs) => {
  const out = { ...values };
  for (const spec of specs ?? []) {
    if (out[spec.key] !== undefined && out[spec.key] !== null) continue;
    if (spec.default !== undefined && spec.default !== null) out[spec.key] = spec.default;
    else if (spec.type === "flag") out[spec.key] = false;
    else delete out[spec.key];
  }
  return out;
};

const begin = (plugin, req) => {
  const controller = new AbortController();
  run = {
    pluginId: plugin.id,
    library: req.library,
    assets: new Map(),
    selection: [],
    writeJail: null,
    defaults: { tags: req.defaults?.tags ?? [], provenance: req.defaults?.provenance ?? {} },
    committed: [],
    controller,
  };
  return run;
};

const end = () => {
  run = null;
};

const makeCommit = (plugin) => async (entries) => {
  if (!Array.isArray(entries)) throw new Error("commit expects a list of entries");
  const library = run.library.id;
  const prepared = entries.map((entry) => unqualifyEntry(library, withOrigin(withDefaults(entry, run.defaults), plugin.id, plugin.requiredBy)));
  // The ids of the items now in the library (an animation source is listed by its clips), not the entries' own.
  const ids = call("library.merge", library, prepared);
  for (const id of ids) if (!run.committed.includes(id)) run.committed.push(id);
};

const common = (plugin) => ({
  library: { ...run.library },
  host: plugin.ctx.host,
  fs: plugin.ctx.fs,
  services: plugin.ctx.services,
  extensions,
  commit: makeCommit(plugin),
  log: (message) => call("event", "log", String(message)),
  warn: (message) => call("event", "warn", String(message)),
  signal: run.controller.signal,
});

const strings = (value) => (Array.isArray(value) ? value.map(String) : []);

/** req: { pluginId, name, library, paths, options, optionSpecs, defaults } */
export async function runImporter(req) {
  const plugin = needPlugin(req.pluginId);
  const handler = plugin.module.cli?.importers?.[req.name];
  if (typeof handler !== "function") {
    throw new Error(`plugin '${plugin.id}' declares the importer '${req.name}' in its manifest but its module has no cli.importers["${req.name}"]`);
  }
  begin(plugin, req);
  try {
    const importRun = { ...common(plugin), paths: [...req.paths], options: applyDefaults(req.options, req.optionSpecs) };
    const result = await handler(importRun);
    return {
      imported: [...run.committed],
      skipped: strings(result?.skipped),
      warnings: strings(result?.warnings),
    };
  } finally {
    end();
  }
}

/** req: { pluginId, library, assetIds, assetKinds, targetDir, preserveStructure, options, optionSpecs, previewOnly } */
export async function runExporter(req) {
  const plugin = needPlugin(req.pluginId);
  const module = plugin.module;
  if (typeof module.run !== "function") throw new Error(`plugin '${plugin.id}' declares an exporter but its module has no run function`);
  begin(plugin, req);
  try {
    run.writeJail = req.targetDir;
    run.selection = req.assetIds.map((id) => qualifyId(run.library.id, id));
    await fetchAssets(run.selection);
    const assets = selected();
    if (assets.length !== run.selection.length) {
      const missing = run.selection.filter((id) => !run.assets.has(id));
      throw new Error(`asset not found in the library: ${missing.slice(0, 5).join(", ")}${missing.length > 5 ? ` and ${missing.length - 5} more` : ""}`);
    }
    const kinds = req.assetKinds;
    if (assets.some((a) => !(kinds.includes("*") || kinds.includes(a.kind === "plugin" && a.type ? a.type : a.kind)))) {
      throw new Error("This exporter does not accept the selected asset type.");
    }
    const report = createReportBuilder();
    const config = { variantPolicy: "all", ...applyDefaults(req.options, req.optionSpecs), targetDir: req.targetDir, preserveStructure: !!req.preserveStructure };
    const ctx = { host: plugin.hostApi, fs: plugin.ctx.fs, services: plugin.ctx.services, extensions, config, report, signal: run.controller.signal };
    if (req.previewOnly) {
      if (typeof module.preview !== "function") throw new Error(`plugin '${plugin.id}' cannot preview: its module has no preview function`);
      const paths = await module.preview(ctx, assets);
      return { written: strings(paths), skipped: [], warnings: [] };
    }
    const returned = await module.run(ctx, assets);
    const final = returned && Array.isArray(returned.written) ? returned : report.done();
    return { written: strings(final.written), skipped: strings(final.skipped), warnings: strings(final.warnings) };
  } finally {
    end();
  }
}

/** Plain JSON data of what a command returned; anything else is an error naming where it is. */
const jsonable = (value, path = "the result") => {
  if (value === undefined || value === null) return null;
  switch (typeof value) {
    case "boolean":
    case "string":
      return value;
    case "number":
      if (!Number.isFinite(value)) throw new Error(`${path} is ${value}, which JSON cannot hold`);
      return value;
    case "bigint":
    case "function":
    case "symbol":
      throw new Error(`${path} is a ${typeof value}, which JSON cannot hold`);
  }
  if (typeof value.toJSON === "function") return jsonable(value.toJSON(), path);
  if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer) throw new Error(`${path} is a byte buffer, which JSON cannot hold`);
  if (value instanceof Map || value instanceof Set) throw new Error(`${path} is a ${value instanceof Map ? "Map" : "Set"}, which JSON cannot hold`);
  if (Array.isArray(value)) return value.map((item, i) => jsonable(item, `${path}[${i}]`));
  const out = {};
  for (const [key, item] of Object.entries(value)) {
    if (item === undefined) continue;
    out[key] = jsonable(item, `${path}.${key}`);
  }
  return out;
};

/** req: { pluginId, path, library, args, options, argSpecs, optionSpecs, assetIds } */
export async function runCommand(req) {
  const plugin = needPlugin(req.pluginId);
  const handler = plugin.module.cli?.commands?.[req.path];
  if (typeof handler !== "function") {
    throw new Error(`plugin '${plugin.id}' declares the command '${req.path}' in its manifest but its module has no cli.commands["${req.path}"]`);
  }
  begin(plugin, req);
  try {
    let assets;
    if (req.assetIds) {
      run.selection = req.assetIds.map((id) => qualifyId(run.library.id, id));
      await fetchAssets(run.selection);
      assets = selected();
      if (assets.length !== run.selection.length) {
        const missing = run.selection.filter((id) => !run.assets.has(id));
        throw new Error(`asset not found in the library: ${missing.slice(0, 5).join(", ")}${missing.length > 5 ? ` and ${missing.length - 5} more` : ""}`);
      }
    }
    let printed = false;
    const commandRun = {
      ...common(plugin),
      args: applyDefaults(req.args, req.argSpecs),
      options: applyDefaults(req.options, req.optionSpecs),
      print: (text) => {
        printed = true;
        call("event", "print", String(text));
      },
    };
    if (assets) commandRun.assets = assets;
    const result = await handler(commandRun);
    return { printed, result: jsonable(result) };
  } finally {
    end();
  }
}
