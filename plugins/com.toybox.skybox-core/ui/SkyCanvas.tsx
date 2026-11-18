/**
 * An interactive view from inside a sky: drag to look around, scroll to zoom.
 * Shared by the full viewer and the import preview; the caller says where the
 * sky's files come from.
 */

import { useEffect, useRef, useState } from "react";

import { loadSky, type SkySource } from "../lib/sky.js";
import { SkyStage } from "../lib/stage.js";

interface Props {
  /** Identifies the sky; a new value loads a new one. */
  sourceKey: string;
  source: SkySource;
  exposure?: number;
  spheres?: boolean;
  /** Cap on the panorama width uploaded to the GPU. */
  maxWidth?: number;
}

export function SkyCanvas({ sourceKey, source, exposure = 0, spheres = false, maxWidth }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const stage = useRef<SkyStage | null>(null);
  const frame = useRef(0);
  const [state, setState] = useState<{ kind: "loading" } | { kind: "ready" } | { kind: "error"; message: string }>({ kind: "loading" });
  // The source object is rebuilt by callers on every render; `sourceKey` says
  // when it actually refers to a different sky.
  const latest = useRef(source); latest.current = source;

  const redraw = () => {
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => stage.current?.render());
  };

  // One renderer for the life of the component, sized to its element. The
  // canvas is made here, not in JSX: a canvas whose WebGL context has been
  // released cannot be given a new one, and React may run this effect twice.
  useEffect(() => {
    const parent = host.current;
    if (!parent) return;
    const element = document.createElement("canvas");
    element.tabIndex = 0;
    element.setAttribute("aria-label", "Sky view. Drag or use the arrow keys to look around; scroll or press plus and minus to zoom.");
    Object.assign(element.style, { position: "absolute", inset: "0", width: "100%", height: "100%", display: "block", cursor: "grab", touchAction: "none", outlineOffset: "-2px" });
    parent.prepend(element);
    let created: SkyStage;
    try { created = new SkyStage(element); }
    catch (error) { element.remove(); setState({ kind: "error", message: `3D graphics are unavailable: ${String(error)}` }); return; }
    stage.current = created;
    const resize = () => { created.resize(element.clientWidth, element.clientHeight, Math.min(2, window.devicePixelRatio || 1)); redraw(); };
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    resize();

    let last: { x: number; y: number } | null = null;
    const down = (event: PointerEvent) => { last = { x: event.clientX, y: event.clientY }; element.setPointerCapture(event.pointerId); };
    const move = (event: PointerEvent) => {
      if (!last) return;
      created.drag(event.clientX - last.x, event.clientY - last.y, element.clientHeight);
      last = { x: event.clientX, y: event.clientY };
      redraw();
    };
    const up = (event: PointerEvent) => { last = null; if (element.hasPointerCapture(event.pointerId)) element.releasePointerCapture(event.pointerId); };
    const wheel = (event: WheelEvent) => { event.preventDefault(); created.setFov(created.fov * Math.exp(Math.max(-200, Math.min(200, event.deltaY)) * 0.0012)); redraw(); };
    const keys = (event: KeyboardEvent) => {
      const step = event.shiftKey ? 80 : 24;
      const turn: Record<string, [number, number]> = { ArrowLeft: [step, 0], ArrowRight: [-step, 0], ArrowUp: [0, step], ArrowDown: [0, -step] };
      if (turn[event.key]) { event.preventDefault(); created.drag(turn[event.key][0], turn[event.key][1], element.clientHeight); redraw(); }
      else if (event.key === "+" || event.key === "=") { created.setFov(created.fov * 0.9); redraw(); }
      else if (event.key === "-") { created.setFov(created.fov / 0.9); redraw(); }
    };
    element.addEventListener("pointerdown", down);
    element.addEventListener("pointermove", move);
    element.addEventListener("pointerup", up);
    element.addEventListener("pointercancel", up);
    element.addEventListener("wheel", wheel, { passive: false });
    element.addEventListener("keydown", keys);
    return () => {
      observer.disconnect();
      element.removeEventListener("pointerdown", down);
      element.removeEventListener("pointermove", move);
      element.removeEventListener("pointerup", up);
      element.removeEventListener("pointercancel", up);
      element.removeEventListener("wheel", wheel);
      element.removeEventListener("keydown", keys);
      cancelAnimationFrame(frame.current);
      stage.current = null;
      created.dispose();
      element.remove();
    };
  }, []);

  useEffect(() => {
    const current = stage.current;
    if (!current) return;
    const controller = new AbortController();
    setState({ kind: "loading" });
    loadSky(latest.current, { maxWidth, signal: controller.signal }).then((sky) => {
      if (controller.signal.aborted || stage.current !== current) { sky.dispose(); return; }
      current.setSky(sky);
      current.look(0, 0);
      setState({ kind: "ready" });
      redraw();
    }, (error) => {
      if (controller.signal.aborted) return;
      current.setSky(undefined);
      redraw();
      setState({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    });
    return () => controller.abort();
  }, [sourceKey, maxWidth]);

  useEffect(() => { stage.current?.setExposure(exposure); stage.current?.setSpheres(spheres); redraw(); }, [exposure, spheres, state.kind]);

  return <div ref={host} style={{ position: "relative", width: "100%", height: "100%", minHeight: 0, background: "var(--preview-background, #111)" }}>
    {state.kind !== "ready" && <p role={state.kind === "error" ? "alert" : "status"}
      style={{ position: "absolute", left: 12, bottom: 12, right: 12, margin: 0, padding: "6px 10px", width: "fit-content", maxWidth: "calc(100% - 24px)", borderRadius: 6, background: "var(--floating-surface, rgba(0,0,0,.6))", color: state.kind === "error" ? "var(--danger, #f66)" : "var(--text-1, #ddd)", overflowWrap: "anywhere" }}>
      {state.kind === "loading" ? "Loading sky…" : `This sky could not be shown. ${state.message}`}
    </p>}
  </div>;
}
