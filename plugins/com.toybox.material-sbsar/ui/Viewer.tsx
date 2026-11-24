/**
 * The full view of a substance. The stage is a live preview of what is being
 * edited; the side panel lists the materials made from the substance, and edits
 * one of them — or a new one — with the preset, the texture size and the
 * archive's parameters. Keeping the result renders it at full size: in place
 * ("Update <name>", same id) or as another material of the substance.
 */

import { useCallback, useEffect, useMemo, useReducer, useState, type CSSProperties, type ReactNode } from "react";
import { Button, Select, TextInput } from "@ldlework/toybox-sdk/ui";
import type { AssetView, AssetViewerCtx } from "@ldlework/toybox-sdk";

import { canUpdate, checkNewName, defaultNewName, editReducer, initialEdit, initialTarget, materialCaption, NEW_TARGET, recordOf, sameState, sortByName, startingState, uniqueName, type EditorState, type Target } from "../lib/editor.js";
import { tidyScratch, type Env } from "../lib/engine.js";
import { cleanName, DEFAULT_SIZE, presetValues, SIZES, sizeOf, type Value } from "../lib/graph.js";
import { rerenderMaterial } from "../lib/rerender.js";
import { absolutePath, humanBytes, rawId, readSubstanceMeta, sameId, substanceArchive } from "../lib/substance.js";
import { takeEdit } from "./handoff.js";
import { Parameters, useParameterView } from "./Parameters.js";
import { useLoaded, usePreview } from "./useSubstance.js";

const message = (failure: unknown) => (failure instanceof Error ? failure.message : String(failure));

const styles = {
  block: { padding: "0 8px" } as CSSProperties,
  field: { display: "grid", gap: 6, color: "var(--text-1)" } as CSSProperties,
  // Bounded, so dozens of materials scroll inside it and the editor stays in reach; padded so focus rings show.
  list: { display: "grid", gap: 2, padding: 4, maxHeight: "min(40vh, 360px)", overflowY: "auto", overscrollBehavior: "contain" } as CSSProperties,
  item: { display: "flex", alignItems: "center", gap: 4, borderRadius: 8 } as CSSProperties,
  pick: { flex: 1, minWidth: 0, minHeight: 48, gap: 10, padding: "4px 6px" } as CSSProperties,
  thumb: { width: 40, height: 40, flex: "none", borderRadius: 6, overflow: "hidden", display: "grid", placeItems: "center", background: "var(--bg-2)" } as CSSProperties,
  names: { display: "grid", minWidth: 0, gap: 2 } as CSSProperties,
  name: { overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: "var(--text-0)" } as CSSProperties,
  caption: { overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 12, color: "var(--text-2)" } as CSSProperties,
  // Pinned to the bottom of the panel's scroll area (12 px side padding, none below); being the last block it
  // never covers the last parameter once scrolled to the end.
  actions: { position: "sticky", bottom: 0, zIndex: 1, display: "grid", gap: 8, margin: "16px -12px 0", padding: "12px 20px 14px", borderTop: "1px solid var(--border)", background: "var(--bg-0)" } as CSSProperties,
  label: { overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", minWidth: 0 } as CSSProperties,
  row: { display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" } as CSSProperties,
  note: { margin: 0, color: "var(--text-2)", fontSize: 12, lineHeight: 1.5 } as CSSProperties,
  error: { margin: 0, color: "var(--danger)", overflowWrap: "anywhere" } as CSSProperties,
};

/** One viewer per substance: a different substance starts afresh. */
export function SubstanceViewer({ ctx }: { ctx: AssetViewerCtx }) {
  return <Editor key={ctx.asset.id} ctx={ctx} />;
}

function Editor({ ctx }: { ctx: AssetViewerCtx }) {
  const { asset, setSidebar, dataVersion, widgets } = ctx;
  const env = useMemo<Env>(() => ({ services: ctx.services, fs: ctx.fs }), [ctx.services, ctx.fs]);
  const meta = useMemo(() => readSubstanceMeta(asset.meta), [asset.meta]);
  const archive = substanceArchive(asset);
  const [requested] = useState(() => takeEdit(asset.id));

  // --- where the archive is, and the engine ---------------------------------------------------------------
  const [root, setRoot] = useState<string | null>();
  const [files, setFiles] = useState<"loading" | "ready" | string>("loading");
  const [defaultSize, setDefaultSize] = useState(DEFAULT_SIZE);
  useEffect(() => {
    let alive = true;
    // Only empties the scratch folder when no render of this session is in use (see `tidyScratch`).
    void tidyScratch(env).catch(() => undefined);
    void env.services.settings.get().then((settings) => { if (alive) setDefaultSize(sizeOf(settings.size, DEFAULT_SIZE)); }, () => undefined);
    ctx.host.getLibraryRoot().then((found) => { if (alive) setRoot(found); }, () => { if (alive) setRoot(null); });
    // On a remote library the archive is downloaded (the host has normally done so before this mounts).
    ctx.ensureFiles().then(() => { if (alive) setFiles("ready"); }, (failure) => { if (alive) setFiles(`The archive could not be downloaded: ${message(failure)}`); });
    return () => { alive = false; };
    // Once per substance; `env` and `ctx` are rebuilt by the host on unrelated renders.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // --- the materials made from it -------------------------------------------------------------------------
  const [children, setChildren] = useState<AssetView[]>();
  useEffect(() => {
    let alive = true;
    ctx.host.getDerivedAssets(asset.id).then(
      // An older server ignores the filter, so it is checked here too.
      (found) => { if (alive) setChildren(sortByName(found.filter((item) => item.kind === "material" && sameId(item.origin?.derivedFrom, asset.id)))); },
      () => { if (alive) setChildren([]); },
    );
    return () => { alive = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asset.id, dataVersion]);
  const names = useMemo(() => (children ?? []).map((child) => child.name), [children]);

  // --- what is edited -------------------------------------------------------------------------------------
  // Target, graph and editor values move together through `editReducer` (lib/editor.ts).
  const [edit, dispatch] = useReducer(editReducer, initialEdit);
  const { target, editor, graph: graphIndex } = edit;
  const setEditor = useCallback((next: EditorState) => dispatch({ type: "edit", editor: next }), []);
  const child = target?.kind === "material" ? children?.find((item) => rawId(item.id) === target.id) : undefined;
  const record = useMemo(() => (target?.kind === "material" ? recordOf(child?.origin?.meta) : undefined), [target, child]);

  const select = useCallback((next: Target, from?: AssetView) => dispatch({ type: "select", target: next, record: recordOf(from?.origin?.meta) }), []);
  // The first target: the material asked for by "Open substance", else the first material, else a new one.
  useEffect(() => {
    if (!children || target) return;
    const first = initialTarget(children, requested);
    select(first, first.kind === "material" ? children.find((item) => rawId(item.id) === first.id) : undefined);
  }, [children, target, requested, select]);
  // A material saved as new is adopted once listed; a target material deleted meanwhile leaves a new material.
  useEffect(() => { if (children) dispatch({ type: "children", ids: children.map((item) => item.id) }); }, [children]);

  const file = root && archive && files === "ready" ? absolutePath({ root, path: archive }) : undefined;
  const loaded = useLoaded(env, file, graphIndex);
  const substance = loaded.status === "ready" && loaded.substance.graph.index === graphIndex ? loaded.substance : undefined;
  const baseline = useMemo(() => (substance && target ? startingState(substance.graph, record, record?.size ?? defaultSize) : undefined), [substance, target, record, defaultSize]);
  useEffect(() => { if (baseline) dispatch({ type: "load", baseline }); }, [baseline, editor]);
  const dirty = !!editor && !!baseline && !sameState(editor, baseline);
  const updatable = canUpdate(edit) && !!child;

  const [busy, setBusy] = useState<"update" | "new">();
  const [status, setStatus] = useState("");
  const [error, setError] = useState<string>();
  // While saving, the last preview stays on screen and nothing new is rendered.
  const { preview, rendering, error: previewError } = usePreview(env, substance, editor?.values, !!busy);
  const graph = preview?.graph ?? substance?.graph;
  const base = useMemo(() => (graph && editor ? presetValues(graph, editor.preset) : undefined), [graph, editor?.preset]); // eslint-disable-line react-hooks/exhaustive-deps
  const [view, setView] = useParameterView();

  // --- naming a new material ------------------------------------------------------------------------------
  const [saving, setSaving] = useState(false); // "Save as new material…" opened for a material target
  const [typed, setTyped] = useState<string>();
  const suggested = target?.kind === "material" && child ? uniqueName(`${child.name} variation`, names) : defaultNewName(asset.name, editor?.preset ?? null, names);
  const newName = typed ?? suggested;
  const nameProblem = checkNewName(newName, names);
  useEffect(() => { setSaving(false); setTyped(undefined); }, [target]);

  const change = useCallback((identifier: string, value: Value) => { if (editor) setEditor({ ...editor, values: { ...editor.values, [identifier]: value } }); }, [editor, setEditor]);
  const resetAll = useCallback(() => { if (editor && graph) setEditor({ ...editor, values: presetValues(graph, editor.preset) }); }, [editor, graph, setEditor]);

  const save = async (mode: "update" | "new") => {
    if (!substance || !editor || !root) return;
    if (mode === "update" && !updatable) return;
    const name = mode === "update" ? child?.name ?? "" : cleanName(newName);
    setBusy(mode); setError(undefined); setStatus(`Rendering ${name} at ${2 ** editor.size} px…`);
    try {
      const entry = await rerenderMaterial(env, root, { substance: asset, ...(child ? { material: child } : {}) }, substance,
        { mode, name, preset: editor.preset, values: editor.values, size: editor.size });
      setStatus("Saving to the library…");
      await ctx.commit([entry]);
      setStatus(mode === "update" ? `${name} updated.` : `Saved as "${name}".`);
      // The new material is the target at once (its row may appear a moment later): "Update" then means it.
      if (mode === "new") dispatch({ type: "savedNew", id: entry.id });
      setSaving(false); setTyped(undefined);
    } catch (failure) { setError(message(failure)); setStatus(""); }
    finally { setBusy(undefined); }
  };

  // --- the side panel -------------------------------------------------------------------------------------
  const graphs = substance?.graphs ?? [];
  const facts = [meta?.sbsar, meta?.bytes !== undefined ? humanBytes(meta.bytes) : undefined, children ? `${children.length} ${children.length === 1 ? "material" : "materials"}` : undefined].filter(Boolean).join(" · ");
  const disabled = !!busy || !editor;
  const pending = !!edit.pending && !child;
  const materialRow = (item: AssetView) => {
    const chosen = target?.kind === "material" && target.id === rawId(item.id);
    const caption = [materialCaption(item.origin?.meta), chosen && dirty ? "edited" : ""].filter(Boolean).join(" · ");
    return <div key={item.id} role="listitem" style={{ ...styles.item, background: chosen ? "var(--bg-3)" : undefined }}>
      <button type="button" className="ds-navigation-item" style={styles.pick} aria-pressed={chosen} disabled={!!busy}
        title={chosen ? `Editing ${item.name}; choose it again to undo your changes` : `Edit ${item.name}`} onClick={() => select({ kind: "material", id: rawId(item.id) }, item)}>
        <span style={styles.thumb}><widgets.AssetThumbnail id={item.id} size={40} /></span>
        <span style={styles.names}><span style={styles.name}>{item.name}</span><span style={styles.caption}>{caption}</span></span>
      </button>
      <Button variant="ghost" aria-label={`Open ${item.name}`} title={`Open ${item.name}`} onClick={() => ctx.host.openAsset(item.id)}>Open</Button>
    </div>;
  };
  const newChosen = target?.kind === "new";

  const actions: ReactNode = editor && graph && <div style={styles.actions}>
    {dirty && <div style={styles.row}>
      <span style={{ ...styles.note, flex: 1 }} role="status">Unsaved changes{target?.kind === "material" ? ` to ${child?.name ?? "this material"}` : ""}.</span>
      {target?.kind === "material" && <Button variant="ghost" disabled={!!busy} onClick={() => baseline && dispatch({ type: "revert", baseline })}>Revert</Button>}
    </div>}
    {pending && <p role="status" style={styles.note}>Waiting for the library to list the new material…</p>}
    {target?.kind === "material" && child && !saving && <>
      <Button variant="primary" disabled={!!busy || !dirty || !updatable} style={{ minWidth: 0, justifyContent: "center" }} title={dirty ? `Render ${child.name} again with these settings, in place` : "Change something first"} onClick={() => void save("update")}>
        <span style={styles.label}>{busy === "update" ? "Updating…" : `Update ${child.name}`}</span></Button>
      <Button disabled={!!busy} onClick={() => setSaving(true)}>Save as new material…</Button>
    </>}
    {(newChosen || saving) && <>
      <label style={styles.field}>{newChosen ? "Name of the new material" : "Save as a new material named"}
        <TextInput value={newName} disabled={!!busy} aria-invalid={!!nameProblem}
          onChange={(event) => setTyped(event.currentTarget.value)} onKeyDown={(event) => { if (event.key === "Enter" && !nameProblem) void save("new"); }} /></label>
      {nameProblem && newName.trim() !== "" && <p role="alert" style={styles.error}>{nameProblem}</p>}
      <div style={styles.row}>
        <Button variant="primary" disabled={!!busy || !!nameProblem} style={{ flex: 1 }} onClick={() => void save("new")}>{busy === "new" ? "Creating…" : "Create material"}</Button>
        {saving && <Button variant="ghost" disabled={!!busy} onClick={() => { setSaving(false); setTyped(undefined); }}>Cancel</Button>}
      </div>
    </>}
    <p role="status" style={{ ...styles.note, minHeight: "1.5em" }}>{status}</p>
    {error && <p role="alert" style={styles.error}>{error}</p>}
  </div>;

  useEffect(() => {
    setSidebar(<>
      <h2 className="viewer-panel-title" style={{ overflowWrap: "anywhere" }}>{asset.name}</h2>
      <p className="viewer-panel-subtitle" style={{ overflowWrap: "anywhere" }}>{facts || "Substance archive"}</p>

      <div className="viewer-panel-section">Materials</div>
      <div role="list" aria-label="Materials made from this substance" style={styles.list}>
        <div role="listitem" style={{ ...styles.item, background: newChosen ? "var(--bg-3)" : undefined }}>
          <button type="button" className="ds-navigation-item" style={styles.pick} aria-pressed={newChosen} disabled={!!busy} onClick={() => select(NEW_TARGET)}>
            <span style={{ ...styles.thumb, fontSize: 20, color: "var(--text-1)" }} aria-hidden="true">+</span>
            <span style={styles.names}><span style={styles.name}>New material</span><span style={styles.caption}>Render another material from this substance</span></span>
          </button>
        </div>
        {children === undefined ? <p style={{ ...styles.note, padding: "4px 8px" }} role="status">Looking for its materials…</p> : children.map(materialRow)}
      </div>

      {editor && graph && <>
        <div className="viewer-panel-section">Rendering</div>
        <div style={{ ...styles.block, display: "grid", gap: 12 }}>
          {edit.unknown && <p role="note" style={styles.note}>How this material was rendered is not recorded, so its settings are unknown: the editor starts from the archive's defaults.</p>}
          {graphs.length > 1 && <label style={styles.field}>Graph<Select value={String(graphIndex)} disabled={disabled}
            options={graphs.map((item) => ({ value: String(item.index), label: item.label }))}
            onChange={(value) => dispatch({ type: "graph", graph: Number(value) })} /></label>}
          {graphs.length > 1 && target?.kind === "material" && <p style={styles.note}>A material keeps its graph: choosing another starts a new material of that graph.</p>}
          {graph.presets.length > 0 && <label style={styles.field}>Preset<Select value={editor.preset ?? ""} disabled={disabled}
            options={[{ value: "", label: "None (defaults)" }, ...graph.presets.map((item) => ({ value: item.label, label: item.label }))]}
            onChange={(label) => setEditor({ ...editor, preset: label || null, values: presetValues(graph, label || null) })} /></label>}
          <label style={styles.field}>Texture size<Select value={String(editor.size)} disabled={disabled}
            options={SIZES.map(({ value, label }) => ({ value, label: `${label} × ${label}` }))} onChange={(value) => setEditor({ ...editor, size: Number(value) })} /></label>
        </div>
        <div className="viewer-panel-section">Parameters</div>
        <div style={styles.block}>
          {base && <Parameters graph={graph} values={editor.values} base={base} disabled={disabled} onChange={change} onResetAll={resetAll} view={view} onView={setView} />}
        </div>
      </>}
      {actions}
    </>);
    // Everything the panel shows or its handlers read; `save` and `select` read only these.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [setSidebar, asset, facts, children, target, child, editor, baseline, dirty, graph, base, substance, root, busy, status, error, saving, newName, nameProblem, view, widgets, edit, updatable, pending]);
  useEffect(() => () => setSidebar(null), [setSidebar]);

  // --- the stage ------------------------------------------------------------------------------------------
  const failure = !meta || !archive ? "This substance does not list its archive, so it cannot be opened."
    : typeof files === "string" && files !== "loading" && files !== "ready" ? files
    : root === null ? "This library has no folder on this computer to open the archive from."
    : loaded.status === "error" ? loaded.error
    : !preview ? previewError : undefined;
  const waiting = files === "loading" ? `Downloading ${meta?.sbsar ?? "the archive"}…`
    : loaded.status !== "ready" ? `Opening ${meta?.sbsar ?? "the archive"} in the Substance engine…`
    : "Rendering a preview…";
  const floating = busy ? status : preview && rendering ? "Updating preview…" : preview && previewError ? previewError : "";
  return <>
    {preview && <widgets.MaterialPreview maps={preview.maps} resolveUrl={preview.resolveUrl} sourceKey={preview.key} />}
    {!preview && <div className="viewer__plugin-fallback">
      <widgets.AssetThumbnail id={asset.id} size={160} alt={asset.name} />
      <p role={failure ? "alert" : "status"} style={failure ? { color: "var(--danger)", overflowWrap: "anywhere" } : undefined}>{failure ?? waiting}</p>
    </div>}
    {floating && <p className="viewer__preview-status" role="status">{floating}</p>}
  </>;
}
