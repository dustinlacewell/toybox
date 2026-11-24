/**
 * A client for Adobe's Substance Remote Engine, the program in the Substance
 * 3D Integration Tools that renders `.sbsar` archives. The engine listens on a
 * local port and takes one JSON message per connection.
 *
 * Things learned from the engine itself:
 * - every load is a fresh instance at its default values;
 * - a render request must name **every** output of the graph — asking for a
 *   subset stops the engine;
 * - a render is queued, and reports completion only to an HTTP callback this
 *   plugin has no use for, so the files it writes are watched instead. An
 *   output the archive has switched off gets no file at all;
 * - a reply to a parameter update carries the inputs again, with which of them
 *   are currently visible — but with each "default" replaced by the current
 *   value, so defaults are only trusted from the first load;
 * - on Windows its OpenGL renderer writes images with red and blue swapped;
 *   Direct3D, Vulkan and the processor renderer do not. Direct3D is used there.
 */

import type { FsApi, PluginServices } from "@ldlework/toybox-sdk";

import { outputFile, readGraphs, readInputs, sameValue, withVisibility, type EngineInput, type Graph, type Value, type Values } from "./graph.js";

const PORT = 41646;
const SYSTEM = "Lwnd-CHcT-uJlw-YSw3";
const SUBSTANCE = "m7ef-7gYN-zTGu-4D3w";
const SYNC = 1;
const OP = { addClient: 10, load: 20, update: 24, render: 25 } as const;

/** The settings key of the engine program (see `manifest.json`). */
const ENGINE_SETTING = "engine";
export const SETTINGS_HINT = "Check the Substance engine under Settings → Plugins → Substances (on the command line: tbx sbsar engine --start; set it with tbx plugin settings com.toybox.material-sbsar engine <path>).";

const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

export interface Env {
  services: PluginServices;
  fs: FsApi;
  /** Render with this instead of the plugin's "Render with" setting (`auto`, `VK`, `CPU`), for this run only. It
   *  applies when this run starts the engine; an engine that is already running keeps the renderer it started with. */
  renderer?: string;
  /** Progress and timings, for `tbx`. */
  log?: (message: string) => void;
}

interface Reply { code?: number; message?: string; data?: unknown }

/** One engine per app session, shared by every panel of the plugin. Calls run
 *  one at a time: the engine has one render queue. */
let queue: Promise<unknown> = Promise.resolve();
let pid: number | undefined;
let registered = false;
let runs = 0;
/** What the engine said when this plugin introduced itself, if anything: the nearest it has to a version. */
let hello: unknown;
/** The render folders this process created in the scratch folder and has not released yet (their files may still
 *  be read): `cleanupRenders` removes them, and the scratch folder is never emptied while there are any. */
const made: { scratch: string; folder: string }[] = [];
/** Render folders released where they cannot be removed one by one (the app's `fs` has no `removeTree`): they go
 *  when the scratch folder is next emptied. */
let released = 0;
/** The scratch folder was emptied once by this process (of what earlier sessions left behind). */
let swept = false;
/** Released folders that may pile up before the scratch folder is emptied again. */
const SWEEP_AFTER = 8;

function exclusive<T>(work: () => Promise<T>): Promise<T> {
  const next = queue.then(work, work);
  queue = next.catch(() => undefined);
  return next;
}

async function send(env: Env, endpoint: string, operation: number, data: Record<string, unknown>, timeoutMs = 30_000): Promise<Reply> {
  const reply = (await env.services.local.requestJson(PORT, { type: SYNC, endpoint, operation, data, timer: Date.now() / 1000 }, timeoutMs)) as Reply;
  if (!reply || reply.code !== 0) throw new Error(reply?.message || "The Substance engine refused the request.");
  return reply;
}

/** Start the engine if it is not running, and introduce this app to it. */
async function ensure(env: Env): Promise<number> {
  if (!(await env.services.local.isOpen(PORT))) {
    registered = false;
    const settings = await env.services.settings.get();
    const chosen = env.renderer ?? (typeof settings.renderer === "string" ? settings.renderer : "auto");
    // The window has a `navigator`; `tbx` has none, so the engine program's own path tells the system.
    const nav = typeof navigator === "undefined" ? undefined : navigator;
    const windows = nav ? /Windows/i.test(nav.userAgent) : /^[A-Za-z]:|\\|\.exe$/i.test(typeof settings.engine === "string" ? settings.engine : "");
    // Never OpenGL on Windows (see the header), whatever an old setting says.
    const renderer = chosen === "auto" || (windows && chosen === "OGL") ? (windows ? "DX" : "OGL") : chosen;
    const cores = Math.max(1, Math.min(8, nav?.hardwareConcurrency || 4));
    const started = Date.now();
    try {
      await env.services.tools.launch(ENGINE_SETTING, [`-p=${PORT}`, `-e=${renderer}`, `-c=${cores}`, "-mb=2048"]);
    } catch (failure) {
      throw new Error(`The Substance engine could not be started (${failure instanceof Error ? failure.message : String(failure)}). ${SETTINGS_HINT}`);
    }
    let up = false;
    for (let attempt = 0; attempt < 60 && !up; attempt++) { await wait(250); up = await env.services.local.isOpen(PORT); }
    if (!up) throw new Error(`The Substance engine did not start. ${SETTINGS_HINT}`);
    env.log?.(`Substance engine program up and listening after ${((Date.now() - started) / 1000).toFixed(1)} s (renderer ${renderer}, ${cores} cores)`);
  }
  pid ??= await env.services.tools.hostProcessId();
  if (!registered) {
    // The engine reports finished renders to this address; nothing listens there.
    // An engine that outlived a reload of this page already knows the app and
    // says so with an error; a real problem shows up on the next request.
    const introduced = Date.now();
    hello = (await send(env, SYSTEM, OP.addClient, { app: { pid, host: "127.0.0.1", port: 9, key: "CLI", name: "Toybox" } }).catch(() => undefined))?.data;
    registered = true;
    env.log?.(`Introduced to the engine in ${((Date.now() - introduced) / 1000).toFixed(1)} s`);
  }
  return pid;
}

/** The graph an index (number, or digits) or a label (any case) names. An archive can hold several graphs; the
 *  first is the default. Throws, listing the graphs, when none matches. */
export function chooseGraph(graphs: Graph[], choice: number | string = 0): Graph {
  const list = graphs.map((graph) => `${graph.index}: ${graph.label}`).join(", ");
  if (!graphs.length) throw new Error("The Substance engine loaded the file but reported no graph in it.");
  const text = String(choice).trim();
  const found = /^\d+$/.test(text) ? graphs.find((graph) => graph.index === Number(text)) : graphs.find((graph) => graph.label.toLowerCase() === text.toLowerCase());
  if (!found) throw new Error(`The archive has no graph ${text} (it has ${list}).`);
  return found;
}

/** Ask the engine to load an archive; the reply names its graphs. */
async function load(env: Env, file: string): Promise<Reply> {
  const app = await ensure(env);
  // `normal_format: 1` is the OpenGL convention, the one the library stores.
  return send(env, SUBSTANCE, OP.load, { app: { pid: app }, substance: { name: "toybox", file: file.replace(/\\/g, "/"), $outputsize: [9, 9], normal_format: 1 } }, 300_000);
}

/** A rendered map: the file the engine wrote, inside the scratch folder. */
export interface RenderedFile { identifier: string; file: string }
export interface Rendering {
  /** Absolute path of the plugin's scratch folder. */
  scratch: string;
  /** Scratch-relative folder of this render. */
  folder: string;
  files: RenderedFile[];
}

/** One archive loaded into the engine. */
export class Substance {
  /** `graphs`: every graph of the archive as first loaded (the one being rendered is `graph`). `family`: the
   *  instance of each graph of this one load, so choosing another graph of the archive never loads it again. */
  private constructor(private env: Env, private uuid: string, public graph: Graph, private sent: Values, public readonly graphs: Graph[], private family: Map<number, Substance>) {
    family.set(graph.index, this);
  }

  private static defaults(graph: Graph): Values {
    return Object.fromEntries(graph.inputs.map((input) => [input.identifier, input.value]));
  }

  /** Load `file` (absolute path) into the engine. */
  static open(env: Env, file: string, graphChoice: number | string = 0): Promise<Substance> {
    return exclusive(async () => {
      const started = Date.now();
      const reply = await load(env, file);
      env.log?.(`Loaded ${file.replace(/\\/g, "/").split("/").pop()} in ${((Date.now() - started) / 1000).toFixed(1)} s`);
      const data = (reply.data ?? {}) as { uuid?: unknown };
      const graphs = readGraphs(reply);
      const graph = chooseGraph(graphs, graphChoice);
      if (typeof data.uuid !== "string" || !graph) throw new Error("The Substance engine loaded the file but reported no graph in it.");
      return new Substance(env, data.uuid, graph, Substance.defaults(graph), graphs, new Map());
    });
  }

  /** The same loaded archive rendering another of its graphs (by index or label): the engine renders any graph of
   *  a loaded archive, so nothing is loaded again. Each graph keeps its own parameter state. Throws, listing the
   *  graphs, when none matches. */
  withGraph(choice: number | string): Substance {
    const graph = chooseGraph(this.graphs, choice);
    return this.family.get(graph.index) ?? new Substance(this.env, this.uuid, graph, Substance.defaults(graph), this.graphs, this.family);
  }

  /** Every graph of the archive at `file` (absolute path), with its inputs,
   *  outputs and presets, without keeping an instance to render. */
  static graphs(env: Env, file: string): Promise<Graph[]> {
    return exclusive(async () => {
      const graphs = readGraphs(await load(env, file));
      if (!graphs.length) throw new Error("The Substance engine loaded the file but reported no graph in it.");
      return graphs;
    });
  }

  private input(identifier: string): EngineInput | undefined {
    return this.graph.inputs.find((input) => input.identifier === identifier);
  }

  /** Bring the engine's instance to `values` at `size` (a power of two),
   *  sending only what differs from what it already has. */
  private async apply(app: number, values: Values, size: number): Promise<void> {
    const wanted: Values = { ...values, $outputsize: [size, size] };
    for (const [identifier, value] of Object.entries(wanted)) {
      const input = this.input(identifier);
      if (!input || sameValue(this.sent[identifier], value)) continue;
      const reply = await send(this.env, SUBSTANCE, OP.update, {
        app: { pid: app }, substance: { uuid: this.uuid, graph_idx: this.graph.index },
        input: { id: input.id, identifier, type: input.type, value },
      }, 120_000);
      this.sent[identifier] = value;
      const refreshed = readInputs(((reply.data ?? {}) as { graph?: { inputs?: unknown } }).graph?.inputs);
      this.graph = { ...this.graph, inputs: withVisibility(this.graph.inputs, refreshed) };
    }
  }

  /** Render every output with `values` at `size` into a new scratch folder,
   *  and return the files once they are completely written. */
  render(values: Values, size: number): Promise<Rendering> {
    return exclusive(async () => {
      const app = await ensure(this.env);
      await this.apply(app, values, size);
      const scratch = await this.env.services.scratchDir(false);
      const folder = `render-${Date.now().toString(36)}-${++runs}-${Math.random().toString(36).slice(2, 6)}`;
      // Writing a marker creates the folder; the engine does not.
      made.push({ scratch, folder });
      await this.env.fs.writeText(scratch, `${folder}/.toybox`, "");
      const started = Date.now();
      const outputs = this.graph.outputs.map((output) => output.identifier);
      await send(this.env, SUBSTANCE, OP.render, {
        app: { pid: app, host: "127.0.0.1", port: 9 },
        substance: { uuid: this.uuid, graph_idx: this.graph.index },
        render: { path: `${scratch}/${folder}`, outputs: outputs.map((identifier) => ({ identifier, enabled: true, format: "PNG", bitdepth: "8" })) },
      }, 120_000);
      const files = await this.collect(scratch, folder, outputs, size);
      this.env.log?.(`Rendered ${2 ** size} px in ${((Date.now() - started) / 1000).toFixed(1)} s`);
      return { scratch, folder, files };
    });
  }

  /** Wait for the engine to finish writing. It writes one image per output
   *  that is switched on — an archive can switch some off, so not every output
   *  gets a file — and then a thumbnail. The render is over when every image
   *  present is complete and either all outputs have one, or the thumbnail is
   *  there and nothing new has appeared for a moment, or nothing at all has
   *  appeared for several seconds. */
  private async collect(scratch: string, folder: string, outputs: string[], size: number): Promise<RenderedFile[]> {
    // Larger textures take longer; a 4K set of a heavy graph can take minutes.
    const deadline = Date.now() + 90_000 * Math.max(1, 2 ** (size - 10));
    const whole = new Set<string>();
    let listing = "";
    let since = Date.now();
    for (;;) {
      const names = (await this.env.fs.readDir(scratch, folder)).filter((entry) => !entry.isDir && /\.png$/i.test(entry.name)).map((entry) => entry.name).sort();
      if (names.join("\n") !== listing) { listing = names.join("\n"); since = Date.now(); }
      const found = outputs.flatMap((identifier) => { const file = outputFile(names, identifier, outputs); return file ? [{ identifier, file }] : []; });
      if (found.length) {
        const quiet = Date.now() - since;
        const thumbnail = names.some((name) => /_thumb\.png$/i.test(name));
        if (found.length === outputs.length || (thumbnail && quiet >= 700) || quiet >= 6000) {
          if (await this.complete(scratch, folder, found, whole)) return found;
        }
      }
      if (Date.now() > deadline) throw new Error("The Substance engine did not finish rendering in time.");
      if (!(await this.env.services.local.isOpen(PORT))) { registered = false; throw new Error(`The Substance engine stopped while rendering. ${SETTINGS_HINT}`); }
      await wait(200);
    }
  }

  /** A PNG is complete when it ends with its `IEND` chunk. Files already seen
   *  complete (`whole`) are not read again. */
  private async complete(scratch: string, folder: string, files: RenderedFile[], whole: Set<string>): Promise<boolean> {
    for (const { file } of files) {
      if (whole.has(file)) continue;
      const bytes = await this.env.fs.readBytes(scratch, `${folder}/${file}`);
      const tail = bytes.subarray(Math.max(0, bytes.length - 8), bytes.length - 4);
      if (bytes.length < 16 || String.fromCharCode(...tail) !== "IEND") return false;
      whole.add(file);
    }
    return true;
  }
}

/** Empty the scratch folder, but only at a moment when that cannot take a file from under a reader: in the engine
 *  queue (so no render is being written) and with no render of this process unreleased (so none is being read).
 *  It empties once per session (what an earlier session left, e.g. after a crash) and then whenever enough
 *  released folders have piled up that could not be removed one by one. Panels call it when they open. */
export function tidyScratch(env: Env): Promise<void> {
  return exclusive(async () => {
    if (made.length || (swept && released < SWEEP_AFTER)) return;
    await env.services.scratchDir(true);
    swept = true;
    released = 0;
  });
}

/** Remove the render folders this process made in the scratch folder (also after a failure). Folders of other
 *  processes, such as the app's previews, are never touched. Needs `fs.removeTree`, which only `tbx` has; elsewhere it does nothing. */
export async function cleanupRenders(env: Env): Promise<void> {
  const mine = made.splice(0);
  if (!env.fs.removeTree) return;
  for (const { scratch, folder } of mine) await env.fs.removeTree(scratch, folder).catch(() => undefined);
}

/** Done with one render (its maps are copied or read): remove its folder. Where the host cannot remove a folder
 *  (the app) it is counted, and the scratch folder is emptied by `tidyScratch` once nothing is in use. Releasing
 *  a render twice does nothing. */
export async function releaseRender(env: Env, rendering: Rendering): Promise<void> {
  const at = made.findIndex((item) => item.folder === rendering.folder && item.scratch === rendering.scratch);
  if (at < 0) return;
  made.splice(at, 1);
  if (env.fs.removeTree) { await env.fs.removeTree(rendering.scratch, rendering.folder).catch(() => undefined); return; }
  released++;
  // Not awaited: a caller inside the engine queue would wait on itself.
  if (released >= SWEEP_AFTER) void tidyScratch(env).catch(() => undefined);
}

/** How many renders of this process are not released yet, and how many released ones wait for the scratch folder
 *  to be emptied (for tests). */
export const renderBookkeeping = (): { unreleased: number; released: number; swept: boolean } => ({ unreleased: made.length, released, swept });

export interface EngineStatus {
  /** The program the "Substance engine" setting points at. */
  program: string;
  exists: boolean;
  /** The "Render with" setting, and the one used when it is `auto`. */
  renderer: string;
  rendererUsed: string;
  port: number;
  /** Something already listens on the port. */
  running: boolean;
}

/** Where the engine is expected and whether it can be found, without starting it. */
export async function engineStatus(env: Env): Promise<EngineStatus> {
  const settings = await env.services.settings.get();
  const program = typeof settings.engine === "string" ? settings.engine : "";
  const at = Math.max(program.lastIndexOf("/"), program.lastIndexOf("\\"));
  let exists = false;
  if (program && at > 0) {
    const name = program.slice(at + 1).toLowerCase();
    exists = await env.fs.readDir(program.slice(0, at), ".").then((entries) => entries.some((entry) => !entry.isDir && entry.name.toLowerCase() === name), () => false);
  }
  const renderer = env.renderer ?? (typeof settings.renderer === "string" ? settings.renderer : "auto");
  return { program, exists, renderer, rendererUsed: renderer, port: PORT, running: await env.services.local.isOpen(PORT) };
}

/** Start the engine (or find it running) and report how long that took and what it said when introduced. */
export async function startEngine(env: Env): Promise<{ seconds: number; hello: unknown }> {
  const started = Date.now();
  await exclusive(() => ensure(env));
  return { seconds: (Date.now() - started) / 1000, hello };
}
