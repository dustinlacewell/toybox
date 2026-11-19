/**
 * The viewer tab of a material this plugin made: the preset and parameters it
 * was rendered with, a live preview of changes, and two ways to keep them —
 * update the material in place, or save a new one next to it.
 */

import { useEffect, useMemo, useState } from "react";
import { Button, Select, Stack, TextInput } from "@ldlework/toybox-sdk/ui";
import { defineAssetPanel, type AssetPanelCtx } from "@ldlework/toybox-sdk";

import { clearScratch, type Env } from "../lib/engine.js";
import { absolutePath, archiveFile } from "../lib/archive.js";
import { cleanName, presetValues, readMeta, SIZES, type Value, type Values } from "../lib/graph.js";
import { rerenderMaterial } from "../lib/rerender.js";
import { Parameters } from "./Parameters.js";
import { useLoaded, usePreview } from "./useSubstance.js";

const message = (failure: unknown) => (failure instanceof Error ? failure.message : String(failure));
const hint = { color: "var(--text-2)", margin: 0, lineHeight: 1.5 } as const;

export default defineAssetPanel(function SbsarAssetPanel({ ctx }: { ctx: AssetPanelCtx }) {
  const { asset } = ctx;
  const env = useMemo<Env>(() => ({ services: ctx.services, fs: ctx.fs }), [ctx.services, ctx.fs]);
  const meta = useMemo(() => readMeta(asset.origin?.meta), [asset.origin]);
  const [root, setRoot] = useState<string | null>();
  const [preset, setPreset] = useState<string | null>(meta?.preset ?? null);
  const [values, setValues] = useState<Values>();
  const [size, setSize] = useState(String(meta?.size ?? 11));
  const [name, setName] = useState(`${asset.name} variation`);
  const [busy, setBusy] = useState<"update" | "new">();
  const [status, setStatus] = useState("");
  const [error, setError] = useState<string>();

  useEffect(() => {
    void clearScratch(env).catch(() => undefined);
    void ctx.host.getLibraryRoot().then(setRoot, () => setRoot(null));
  }, [env, ctx.host]);

  // Where the archive is kept is archive.ts's business.
  const archive = meta ? archiveFile(asset, meta) : undefined;
  const file = root && archive ? absolutePath({ root, path: archive }) : undefined;
  const loaded = useLoaded(env, file, meta?.graph ?? 0);
  const substance = loaded.status === "ready" ? loaded.substance : undefined;
  // Start from what the material was rendered with.
  useEffect(() => {
    if (substance && meta && !values) setValues({ ...presetValues(substance.graph, meta.preset), ...meta.inputs });
  }, [substance, meta, values]);
  const { preview, rendering, error: previewError } = usePreview(env, busy ? undefined : substance, values);
  const graph = preview?.graph ?? substance?.graph;

  if (!meta || !archive) return <p style={hint}>This material has no Substance archive stored with it, so it cannot be rendered again here.</p>;

  const save = async (mode: "update" | "new") => {
    if (!substance || !values || !root) return;
    const target = mode === "update" ? asset.name : cleanName(name);
    setBusy(mode); setError(undefined); setStatus(`Rendering ${SIZES.find((item) => item.value === size)?.label ?? ""}…`);
    try {
      const entry = await rerenderMaterial(env, root, asset, meta, substance, { mode, name: target, preset, values, size: Number(size) });
      setStatus("Saving to the library…");
      await ctx.commit([entry]);
      setStatus(mode === "update" ? "Material updated." : `Saved as "${target}".`);
    } catch (failure) { setError(message(failure)); setStatus(""); }
    finally { setBusy(undefined); }
  };

  const failure = loaded.status === "error" ? loaded.error : previewError;
  return <Stack gap={12} style={{ padding: "4px 0 16px" }}>
    <p style={hint}>Rendered from <strong style={{ color: "var(--text-1)", overflowWrap: "anywhere" }}>{meta.sbsar}</strong>. Change the preset or parameters to see the result, then keep it.</p>
    <div style={{ position: "relative", aspectRatio: "1 / 1", minHeight: 260, width: "100%", borderRadius: 8, overflow: "hidden", border: "1px solid var(--border)" }}>
      {preview ? <ctx.widgets.MaterialPreview maps={preview.maps} resolveUrl={preview.resolveUrl} sourceKey={preview.key} />
        : <p role={failure ? "alert" : "status"} style={{ margin: 0, padding: 12, color: failure ? "var(--danger)" : "var(--text-2)" }}>
          {failure ?? (root === null ? "No library is open." : loaded.status === "ready" ? "Rendering a preview…" : "Opening in the Substance engine…")}</p>}
    </div>
    <p role="status" style={{ ...hint, minHeight: "1.5em" }}>{status || (preview && rendering ? "Updating preview…" : preview && previewError ? previewError : "")}</p>
    {error && <p role="alert" style={{ margin: 0, color: "var(--danger)", overflowWrap: "anywhere" }}>{error}</p>}
    {graph && values && <>
      {graph.presets.length > 0 && <label>Preset<Select value={preset ?? ""} disabled={!!busy}
        options={[{ value: "", label: "None (defaults)" }, ...graph.presets.map((item) => ({ value: item.label, label: item.label }))]}
        onChange={(label) => { setPreset(label || null); setValues(presetValues(graph, label || null)); }} /></label>}
      <Parameters graph={graph} values={values} disabled={!!busy}
        onChange={(identifier: string, value: Value) => setValues((current) => ({ ...current, [identifier]: value }))} />
      <Button variant="ghost" disabled={!!busy} onClick={() => setValues(presetValues(graph, preset))}>Reset parameters</Button>
      <label>Texture size<Select value={size} disabled={!!busy} options={SIZES.map(({ value, label }) => ({ value, label: `${label} × ${label}` }))} onChange={setSize} /></label>
      <Button variant="primary" disabled={!!busy} onClick={() => void save("update")}>{busy === "update" ? "Updating…" : "Update this material"}</Button>
      <label>Or save as a new material<TextInput aria-label="New material name" value={name} disabled={!!busy} onChange={(event) => setName(event.currentTarget.value)} /></label>
      <Button disabled={!!busy || !name.trim() || cleanName(name).toLowerCase() === asset.name.toLowerCase()} onClick={() => void save("new")}>{busy === "new" ? "Saving…" : "Save as new"}</Button>
    </>}
  </Stack>;
});
