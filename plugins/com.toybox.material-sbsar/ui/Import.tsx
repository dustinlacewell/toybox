/**
 * Import Substance archives as materials. Pick a folder of `.sbsar` files,
 * choose a preset and adjust the parameters of each with a live preview, then
 * render them at full size into the library. The archive is stored in each
 * material's folder, so it can be rendered again later with other settings.
 */

import { useEffect, useMemo, useState } from "react";
import { Button, Checkbox, Pane, Select, Stack, TextInput, Workbench } from "@ldlework/toybox-sdk/ui";
import { defineImportPanel, validSegment, type ImportPanelCtx } from "@ldlework/toybox-sdk";

import { clearScratch, Substance, type Env } from "../lib/engine.js";
import { DEFAULT_SIZE, presetValues, SIZES, sizeOf, type Value } from "../lib/graph.js";
import { findArchives, importArchives, stemOf, type ArchiveConfig as Config } from "../lib/import.js";
import { Parameters } from "./Parameters.js";
import { useLoaded, usePreview } from "./useSubstance.js";

const message = (failure: unknown) => (failure instanceof Error ? failure.message : String(failure));

export default defineImportPanel(function SbsarImport({ ctx }: { ctx: ImportPanelCtx }) {
  const env = useMemo<Env>(() => ({ services: ctx.services, fs: ctx.fs }), [ctx.services, ctx.fs]);
  const [source, setSource] = useState<string>();
  const [archives, setArchives] = useState<string[]>([]);
  const [selected, setSelected] = useState(new Set<string>());
  const [active, setActive] = useState<string>();
  const [configs, setConfigs] = useState(new Map<string, Config>());
  const [category, setCategory] = useState("substance");
  const [size, setSize] = useState(String(DEFAULT_SIZE));
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState<string>();
  const [warnings, setWarnings] = useState<string[]>([]);

  useEffect(() => {
    void clearScratch(env).catch(() => undefined);
    void env.services.settings.get().then((settings) => setSize(String(sizeOf(settings.size, DEFAULT_SIZE))), () => undefined);
  }, [env]);

  const loaded = useLoaded(env, source && active ? `${source}/${active}` : undefined);
  const substance = loaded.status === "ready" ? loaded.substance : undefined;
  const config = active ? configs.get(active) : undefined;
  // An archive opened for the first time starts at its defaults.
  useEffect(() => {
    if (!active || !substance || configs.has(active)) return;
    setConfigs((current) => new Map(current).set(active, { preset: null, values: presetValues(substance.graph, null), everyPreset: false }));
  }, [active, substance, configs]);
  const { preview, rendering, error: previewError } = usePreview(env, busy ? undefined : substance, config?.values);
  const graph = preview?.graph ?? substance?.graph;

  const edit = (change: (config: Config) => Config) => {
    if (!active) return;
    setConfigs((current) => { const existing = current.get(active); return existing ? new Map(current).set(active, change(existing)) : current; });
  };

  const choose = async () => {
    const folder = await ctx.fs.pickDirectory();
    if (!folder) return;
    setBusy(true); setError(undefined); setWarnings([]); setStatus("Looking for Substance archives…");
    try {
      const found = await findArchives(ctx.fs, folder);
      setSource(folder); setArchives(found); setConfigs(new Map());
      setSelected(new Set(found)); setActive(found[0]);
      setStatus(found.length ? `${found.length} ${found.length === 1 ? "archive" : "archives"} found` : "No .sbsar files in this folder.");
    } catch (failure) { setError(message(failure)); setStatus(""); }
    finally { setBusy(false); }
  };

  const run = async () => {
    if (!source) return;
    setBusy(true); setError(undefined); setWarnings([]);
    try {
      const root = await ctx.host.getLibraryRoot();
      if (!root) throw new Error("Choose a library first");
      const chosen = archives.filter((path) => selected.has(path));
      const { entries, problems } = await importArchives({
        env, libraryRoot: root, source, archives: chosen, category, size: sizeOf(size), progress: setStatus,
        open: async (path) => (path === active && substance ? substance : Substance.open(env, `${source}/${path}`)),
        configOf: (path) => configs.get(path),
      });
      setWarnings(problems);
      if (!entries.length) throw new Error(problems.length ? "Nothing could be rendered." : "Nothing is selected.");
      setStatus("Adding materials to the library…");
      await ctx.commit(entries);
      setStatus(`Added ${entries.length} ${entries.length === 1 ? "material" : "materials"}.`);
    } catch (failure) { setError(message(failure)); setStatus(""); }
    finally { setBusy(false); }
  };

  const ready = !!selected.size && validSegment(category);
  useEffect(() => {
    ctx.setActions?.(<>
      <Button disabled={busy} onClick={() => void choose()}>{source ? "Change folder" : "Choose folder"}</Button>
      <Button variant="primary" disabled={busy || !ready} onClick={() => void run()}>{busy ? "Working…" : `Import ${selected.size || "materials"}`}</Button>
    </>);
    return () => ctx.setActions?.(null);
    // The handlers read current state; re-registering on each change keeps them fresh.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ctx.setActions, source, archives, selected, configs, category, size, busy, active, substance]);

  const hint = { color: "var(--text-2)", margin: 0 } as const;
  return <Workbench navigation={<Pane><Stack gap={16} style={{ padding: 16 }}>
    <h3>Import into</h3>
    <label>Category<TextInput aria-label="Category" value={category} disabled={busy} onChange={(event) => setCategory(event.currentTarget.value)} /></label>
    {!validSegment(category) && <p role="alert" style={{ color: "var(--danger)", margin: 0 }}>Enter a category name without slashes or other reserved characters.</p>}
    <label>Texture size<Select value={size} disabled={busy} options={SIZES.map(({ value, label }) => ({ value, label: `${label} × ${label}` }))} onChange={setSize} /></label>
    <p style={hint}>Each archive is rendered by Adobe's Substance engine into colour, normal, roughness, metal, shadowing and height maps. The preview is 512 pixels; the import uses the size above.</p>
    <p style={hint}>The archive is stored with each material, so it can be changed later from its own page.</p>
    {source && <p style={{ ...hint, overflowWrap: "anywhere" }}>{source}</p>}
  </Stack></Pane>} inspector={active && <Pane><Stack gap={12} style={{ padding: 16 }}>
    <strong style={{ overflowWrap: "anywhere" }}>{stemOf(active)}</strong>
    <div style={{ position: "relative", aspectRatio: "1 / 1", minHeight: 260, width: "100%", borderRadius: 8, overflow: "hidden", border: "1px solid var(--border)" }}>
      {preview ? <ctx.widgets.MaterialPreview maps={preview.maps} resolveUrl={preview.resolveUrl} sourceKey={preview.key} />
        : <p role={loaded.status === "error" || previewError ? "alert" : "status"} style={{ margin: 0, padding: 12, color: loaded.status === "error" || previewError ? "var(--danger)" : "var(--text-2)" }}>
          {loaded.status === "error" ? loaded.error : previewError ?? (loaded.status === "loading" ? "Opening in the Substance engine…" : "Rendering a preview…")}</p>}
    </div>
    <p role="status" style={{ ...hint, minHeight: "1.4em" }}>{preview && (rendering ? "Updating preview…" : previewError ?? "Drag the preview to look around.")}</p>
    {graph && config && <>
      {graph.presets.length > 0 && <>
        <label>Preset<Select value={config.preset ?? ""} disabled={busy || config.everyPreset}
          options={[{ value: "", label: "None (defaults)" }, ...graph.presets.map((preset) => ({ value: preset.label, label: preset.label }))]}
          onChange={(label) => edit((current) => ({ ...current, preset: label || null, values: presetValues(graph, label || null) }))} /></label>
        <Checkbox checked={config.everyPreset} label={`Import all ${graph.presets.length} presets as separate materials`}
          onChange={(checked) => { if (!busy) edit((current) => ({ ...current, everyPreset: checked })); }} />
      </>}
      {config.everyPreset ? <p style={hint}>Each preset is imported as the archive's author made it.</p>
        : <Parameters graph={graph} values={config.values} disabled={busy}
            onChange={(identifier: string, value: Value) => edit((current) => ({ ...current, values: { ...current.values, [identifier]: value } }))} />}
      {!config.everyPreset && <Button variant="ghost" disabled={busy} onClick={() => edit((current) => ({ ...current, values: presetValues(graph, current.preset) }))}>Reset parameters</Button>}
    </>}
  </Stack></Pane>}>
    <Stack grow style={{ overflow: "hidden" }}>
      <div style={{ padding: "16px 20px", borderBottom: "1px solid var(--border)" }}>
        {archives.length > 0 && <Checkbox checked={selected.size === archives.length} label={`${selected.size} of ${archives.length} selected`}
          onChange={(checked) => { if (!busy) setSelected(checked ? new Set(archives) : new Set()); }} />}
        <p role="status" style={{ color: "var(--text-2)", marginBottom: 0 }}>{status || "Choose a folder of .sbsar files to get started."}</p>
      </div>
      {error && <p role="alert" style={{ padding: "0 20px", color: "var(--danger)" }}>{error}</p>}
      {warnings.map((warning) => <p key={warning} role="alert" style={{ padding: "0 20px", margin: "8px 0 0", color: "var(--danger)" }}>{warning}</p>)}
      <div style={{ overflow: "auto", flex: 1 }}>{archives.map((path) => {
        const own = configs.get(path);
        return <div key={path} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 20px", borderBottom: "1px solid var(--border)", background: active === path ? "var(--bg-2)" : undefined }}>
          <Checkbox checked={selected.has(path)} label="" onChange={(checked) => { if (!busy) setSelected((current) => { const next = new Set(current); if (checked) next.add(path); else next.delete(path); return next; }); }} />
          <Button variant="ghost" disabled={busy} onClick={() => setActive(path)} style={{ flex: 1, justifyContent: "start", minWidth: 0 }} title={path}>
            <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{stemOf(path)}</span>
          </Button>
          <span style={{ color: "var(--text-2)", fontSize: 12, whiteSpace: "nowrap" }}>{own ? (own.everyPreset ? "Every preset" : own.preset ?? "Defaults") : ""}</span>
        </div>;
      })}</div>
    </Stack>
  </Workbench>;
});
