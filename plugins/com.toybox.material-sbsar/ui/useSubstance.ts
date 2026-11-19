/**
 * React glue over the engine: load an archive, and keep a low-resolution
 * preview of it current as its parameters change.
 */

import { useEffect, useRef, useState } from "react";
import type { MaterialMapInput } from "@ldlework/toybox-sdk";

import { Substance, type Env } from "../lib/engine.js";
import { channelOf, type Graph, type Values } from "../lib/graph.js";
import { renderedMaps } from "../lib/produce.js";

export type Loaded =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; substance: Substance }
  | { status: "error"; error: string };

const message = (failure: unknown) => (failure instanceof Error ? failure.message : String(failure));

/** The archive at `file` (absolute path) loaded into the engine. */
export function useLoaded(env: Env, file: string | undefined, graph = 0): Loaded {
  const [state, setState] = useState<{ file?: string; loaded: Loaded }>({ loaded: { status: "idle" } });
  useEffect(() => {
    if (!file) { setState({ loaded: { status: "idle" } }); return; }
    let alive = true;
    setState({ file, loaded: { status: "loading" } });
    Substance.open(env, file, graph).then(
      (substance) => { if (alive) setState({ file, loaded: { status: "ready", substance } }); },
      (failure) => { if (alive) setState({ file, loaded: { status: "error", error: message(failure) } }); },
    );
    return () => { alive = false; };
    // `env` is rebuilt by the host on unrelated renders; its services are stable.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file, graph]);
  // A file that changed this render has not started loading yet.
  return state.file === file ? state.loaded : file ? { status: "loading" } : { status: "idle" };
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

/** A 512-pixel render of `substance` with `values`, redone shortly after they
 *  change. `rendering` is true while a newer one is on its way. */
export function usePreview(env: Env, substance: Substance | undefined, values: Values | undefined): { preview?: Preview; rendering: boolean; error?: string } {
  const [state, setState] = useState<{ owner?: Substance; preview?: Preview; error?: string }>({});
  const [rendering, setRendering] = useState(false);
  const urls = useRef<string[]>([]);
  const release = () => { for (const url of urls.current) URL.revokeObjectURL(url); urls.current = []; };
  useEffect(() => release, []);

  useEffect(() => {
    if (!substance || !values) return;
    let alive = true;
    setRendering(true);
    const timer = setTimeout(async () => {
      try {
        const rendering = await substance.render(values, PREVIEW_SIZE);
        if (!alive) return;
        const files = renderedMaps(substance, rendering);
        const made = new Map<string, string>();
        for (const { file } of files) {
          const bytes = await env.fs.readBytes(rendering.scratch, `${rendering.folder}/${file}`);
          made.set(file, URL.createObjectURL(new Blob([Uint8Array.from(bytes)], { type: "image/png" })));
        }
        if (!alive) { for (const url of made.values()) URL.revokeObjectURL(url); return; }
        release();
        urls.current = [...made.values()];
        setState({
          owner: substance,
          preview: {
            key: rendering.folder,
            maps: files.map(({ role, file }) => { const channel = channelOf(role); return channel ? { role, file, channel } : { role, file }; }),
            resolveUrl: async (file) => made.get(file) ?? "",
            graph: substance.graph,
          },
        });
      } catch (failure) {
        if (alive) setState((current) => ({ ...current, owner: substance, error: message(failure) }));
      } finally {
        if (alive) setRendering(false);
      }
    }, 300);
    return () => { alive = false; clearTimeout(timer); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [substance, values]);

  // A preview of another archive is not this one's.
  return state.owner === substance ? { preview: state.preview, rendering, error: state.error } : { rendering };
}
