/**
 * React glue over the engine: load an archive, and keep a low-resolution
 * preview of it current as its parameters change.
 */

import { useEffect, useRef, useState } from "react";
import type { MaterialMapInput } from "@ldlework/toybox-sdk";

import { releaseRender, Substance, type Env } from "../lib/engine.js";
import { channelOf, type Graph, type Values } from "../lib/graph.js";
import { renderedMaps } from "../lib/produce.js";

export type Loaded =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; substance: Substance }
  | { status: "error"; error: string };

const message = (failure: unknown) => (failure instanceof Error ? failure.message : String(failure));

/** The archive at `file` (absolute path) loaded into the engine, rendering `graph`. Each file is loaded once for the
 *  life of the component: choosing another graph, or coming back to a file, reuses that load (an archive can be
 *  hundreds of megabytes, and the engine offers no way to unload one). A failed load is tried again next time. */
export function useLoaded(env: Env, file: string | undefined, graph = 0): Loaded {
  const [state, setState] = useState<{ file?: string; graph?: number; loaded: Loaded }>({ loaded: { status: "idle" } });
  const cache = useRef(new Map<string, Promise<Substance>>());
  useEffect(() => {
    if (!file) { setState({ loaded: { status: "idle" } }); return; }
    let alive = true;
    let load = cache.current.get(file);
    if (!load) {
      load = Substance.open(env, file, 0);
      cache.current.set(file, load);
      load.catch(() => { if (cache.current.get(file) === load) cache.current.delete(file); });
    }
    setState({ file, graph, loaded: { status: "loading" } });
    load.then((first) => first.withGraph(graph)).then(
      (substance) => { if (alive) setState({ file, graph, loaded: { status: "ready", substance } }); },
      (failure) => { if (alive) setState({ file, graph, loaded: { status: "error", error: message(failure) } }); },
    );
    return () => { alive = false; };
    // `env` is rebuilt by the host on unrelated renders; its services are stable.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file, graph]);
  // A file or graph that changed this render has not started loading yet.
  return state.file === file && state.graph === graph ? state.loaded : file ? { status: "loading" } : { status: "idle" };
}

export interface Preview {
  /** Changes with every render, so the preview reloads its textures. */
  key: string;
  maps: MaterialMapInput[];
  resolveUrl(file: string): Promise<string>;
  /** The graph after the render: which inputs are visible can change. */
  graph: Graph;
}

const PREVIEW_SIZE = 9; // 512 × 512

/** A 512-pixel render of `substance` with `values`, redone shortly after they change. `rendering` is true while a
 *  newer one is on its way. With `paused` (e.g. while a full-size render is being saved) nothing new is scheduled
 *  and the last preview stays; when it ends, a render is made only if the values changed meanwhile. Each render's
 *  folder is released as soon as its maps are read, or when it is no longer wanted. */
export function usePreview(env: Env, substance: Substance | undefined, values: Values | undefined, paused = false): { preview?: Preview; rendering: boolean; error?: string } {
  const [state, setState] = useState<{ owner?: Substance; preview?: Preview; error?: string }>({});
  const [rendering, setRendering] = useState(false);
  const urls = useRef<string[]>([]);
  /** What the preview on screen (or the error) was made from. */
  const shown = useRef<{ substance?: Substance; values?: Values }>({});
  const release = () => { for (const url of urls.current) URL.revokeObjectURL(url); urls.current = []; };
  useEffect(() => release, []);

  useEffect(() => {
    if (!substance || !values || paused) { setRendering(false); return; }
    if (shown.current.substance === substance && shown.current.values === values) { setRendering(false); return; }
    let alive = true;
    setRendering(true);
    const timer = setTimeout(async () => {
      try {
        const done = await substance.render(values, PREVIEW_SIZE);
        const made = new Map<string, string>();
        let files: ReturnType<typeof renderedMaps> = [];
        try {
          if (!alive) return;
          files = renderedMaps(substance, done);
          for (const { file } of files) {
            const bytes = await env.fs.readBytes(done.scratch, `${done.folder}/${file}`);
            made.set(file, URL.createObjectURL(new Blob([Uint8Array.from(bytes)], { type: "image/png" })));
          }
        } finally {
          await releaseRender(env, done);
        }
        if (!alive) { for (const url of made.values()) URL.revokeObjectURL(url); return; }
        release();
        urls.current = [...made.values()];
        shown.current = { substance, values };
        setState({
          owner: substance,
          preview: {
            key: done.folder,
            maps: files.map(({ role, file }) => { const channel = channelOf(role); return channel ? { role, file, channel } : { role, file }; }),
            resolveUrl: async (file) => made.get(file) ?? "",
            graph: substance.graph,
          },
        });
      } catch (failure) {
        if (alive) { shown.current = { substance, values }; setState((current) => ({ ...current, owner: substance, error: message(failure) })); }
      } finally {
        if (alive) setRendering(false);
      }
    }, 300);
    return () => { alive = false; clearTimeout(timer); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [substance, values, paused]);

  // A preview of another archive is not this one's.
  return state.owner === substance ? { preview: state.preview, rendering, error: state.error } : { rendering };
}
