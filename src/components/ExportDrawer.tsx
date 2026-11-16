import { useCallback, useEffect, useMemo, useState } from "react";
import { Button, Checkbox, Select, Stack, TextInput, Workbench, PageHeader, Pane, NavigationGroup, NavigationItem } from "@ldlework/toybox-sdk/ui";
import type { PluginConfig } from "@ldlework/toybox-sdk";
import type { AssetKind } from "../domain/catalog";
import type { SavedExport } from "../domain/savedExport";
import { useSavedExports } from "../state/savedExports";
import { useStore } from "../state/store";
import { buildExportCtx, selectedAssets } from "../services/pluginHost";
import { runExporter, type LoadedExporter, type PluginLoadError } from "../services/pluginRegistry";
import { pickDirectory } from "../services/pickDirectory";
import { assetFingerprints, type ExportReport } from "../services/tauriApi";
import { ExportPanelHost } from "./plugin-ui/ExportPanelHost";
import "./ExportDrawer.css";
import { useThumbUrl } from "./useThumbUrl";
import type { Asset } from "../domain/catalog";
import { sourceLabel } from "../domain/facets";
import { PluginPicker } from "./PluginPicker";

interface Props {
  open: boolean;
  kind: AssetKind;
  selectedIds: string[];
  exporters: LoadedExporter[];
  pluginErrors: PluginLoadError[];
  setupId?: string;
  onClose: () => void;
}
export function ExportDrawer(props: Props) {
  if (!props.open) return null;
  return <ExportSession key={`${props.kind}:${props.setupId ?? "new"}`} {...props} />;
}
function ExportSession({ kind, selectedIds, exporters, pluginErrors, setupId, onClose }: Props) {
  const { exports: saved, save, remove } = useSavedExports();
  const libraryRoot = useStore((s) => s.catalog?.libraryRoot ?? "");
  const setups = saved.filter((s) => s.kind === kind && s.libraryRoot === libraryRoot);
  const [currentId, setCurrentId] = useState(setupId);
  const initial = setups.find((s) => s.id === setupId);
  const available = exporters.filter((e) => (e.manifest.assetKinds ?? ["model"]).includes(kind));
  const [exporterId, setExporterId] = useState(initial?.exporterId ?? "");
  const [targetDir, setTargetDir] = useState(initial?.targetDir ?? "");
  const [preserve, setPreserve] = useState(initial?.preserveStructure ?? true);
  const [policy, setPolicy] = useState<"all" | "best">(initial?.variantPolicy ?? "all");
  const [name, setName] = useState(initial?.name ?? "");
  const [initialConfig, setInitialConfig] = useState(initial?.config);
  const [panelKey, setPanelKey] = useState(0);
  const [panelConfig, setPanelConfig] = useState<Record<string, unknown>>(initial?.config ?? {});
  const [panelReady, setPanelReady] = useState(false);
  const [running, setRunning] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<ExportReport | null>(null);
  const [plan, setPlan] = useState<{ files: string[]; error?: string } | null>(null);
  const active = available.find((e) => e.manifest.id === exporterId);
  const current = setups.find((s) => s.id === currentId);
  const onPanelConfig = useCallback((values: Record<string, unknown>, ready: boolean) => {
    setPanelConfig((old) => JSON.stringify(old) === JSON.stringify(values) ? old : values);
    setPanelReady(ready);
  }, []);
  const config: PluginConfig = useMemo(() => ({ ...panelConfig, targetDir, preserveStructure: preserve, variantPolicy: policy }), [panelConfig, targetDir, preserve, policy]);
  const assets = useStore((s) => s.catalog?.assets);
  const selected = useMemo(() => (assets ?? []).filter((a) => selectedIds.includes(a.id) && a.kind === kind), [assets, selectedIds.join("|"), kind]);
  const ids = useMemo(() => selected.map((a) => a.id), [selected]);
  const [fingerprints, setFingerprints] = useState<Record<string, string> | null>(null);
  useEffect(() => {
    let alive = true;
    setFingerprints(null);
    if (currentId && ids.length) void assetFingerprints(ids, policy === "all").then(
      (value) => { if (alive) setFingerprints(value); },
      (e) => { if (alive) setError(String(e)); });
    return () => { alive = false; };
  }, [currentId, ids, policy]);
  const added = selected.filter((a) => !current?.lastAssets?.[a.id]).length;
  const changed = selected.filter((a) => fingerprints && current?.lastAssets?.[a.id] && current.lastAssets[a.id] !== fingerprints[a.id]).length;
  useEffect(() => {
    setPlan(null); setReport(null);
    if (!active || !panelReady || !targetDir || !ids.length) return;
    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      if (!active.plugin.preview) {
        setPlan({ files: [], error: "This plugin needs a file preview before it can send. Update the plugin to support preview()." }); return;
      }
      active.plugin.preview(buildExportCtx(active.manifest, config, controller.signal), selectedAssets(ids)).then(
        (files) => { if (!controller.signal.aborted) setPlan({ files }); },
        (e) => { if (!controller.signal.aborted) setPlan({ files: [], error: String(e) }); });
    }, 150);
    return () => { controller.abort(); window.clearTimeout(timer); };
  }, [active, config, panelReady, ids]);
  const choose = (id: string) => {
    const s = setups.find((s) => s.id === id);
    setCurrentId(s?.id); setExporterId(s?.exporterId ?? "");
    setTargetDir(s?.targetDir ?? ""); setPreserve(s?.preserveStructure ?? true);
    setPolicy(s?.variantPolicy ?? "all"); setName(s?.name ?? "");
    setInitialConfig(s?.config); setPanelConfig(s?.config ?? {}); setPanelReady(false);
    setPanelKey((k) => k + 1); setError(null); setPlan(null); setReport(null);
  };
  const setup = (): SavedExport => ({ ...current, id: currentId ?? crypto.randomUUID(), name: name.trim(), kind, libraryRoot, exporterId, targetDir, preserveStructure: preserve, config: panelConfig, variantPolicy: policy });
  const saveSetup = async () => {
    setSaving(true); setError(null);
    try { const next = setup(); await save(next); setCurrentId(next.id); }
    catch (e) { setError(String(e)); } finally { setSaving(false); }
  };
  const send = async () => {
    if (!active || !plan || plan.error) return;
    setRunning(true); setError(null); setReport(null);
    try {
      const result = await runExporter(active, ids, config, new AbortController().signal);
      setReport(result);
      if (currentId && fingerprints) await save({ ...setup(), lastSent: new Date().toISOString(), lastAssets: { ...current?.lastAssets, ...fingerprints } });
    } catch (e) { setError(String(e)); } finally { setRunning(false); }
  };
  const choosePlugin = (id: string) => { if(!id)return; setExporterId(id); setInitialConfig(undefined); setPanelConfig({}); setPanelReady(false); setPlan(null); setReport(null); setError(null); setPanelKey((key) => key + 1); };
  return <div className="export-screen" role="dialog" aria-modal="true" aria-label="Export">
    <Workbench navigation={<><div className="library-brand">Toybox</div><NavigationGroup label="Saved exports"><NavigationItem label="New export" onPress={() => choose("")} />{setups.map((s) => <NavigationItem key={s.id} label={s.name} active={s.id === currentId} onPress={() => choose(s.id)} disabled={running} />)}</NavigationGroup></>}
      header={<PageHeader leading={<Button variant="ghost" onClick={onClose} disabled={running}>← Back</Button>} title={current?.name ?? "Export"}
        metadata={exporterId&&<label>Export plugin <Select disabled={running} value={active?exporterId:""} options={[...(!active?[{value:"",label:"Plugin unavailable — choose another"}]:[]),...available.map((e) => ({ value: e.manifest.id, label: e.manifest.name }))]} onChange={choosePlugin} /></label>}>
        <span className="export-screen__target" title={targetDir}>{targetDir}</span>
        {exporterId&&<Button variant="primary" disabled={running || !plan || !!plan.error || !plan.files.length || (!!currentId && !fingerprints)} onClick={() => void send()}>{running ? "Sending…" : `Send ${selected.length || ""}`}</Button>}
      </PageHeader>}
      inspector={exporterId&&<Pane><fieldset className="export-screen__settings" disabled={running || saving}>
        <h2>{active?.manifest.name ?? "Export plugin"}</h2><p className="export__hint">{current ? `Saved with ${current.name}.` : "Choose settings for this export."}</p>
        {!active && <p className="export__error">The saved plugin is unavailable for this type.</p>}
        <p className="export__hint">{active?.manifest.description}</p>
        {active && <ExportPanelHost key={`${exporterId}:${panelKey}`} plugin={active} initialConfig={initialConfig} shared={{ targetDir, preserveStructure: preserve }} onConfig={onPanelConfig} />}
        <h3>Folder</h3><Button onClick={async () => { const folder = await pickDirectory(); if (folder) setTargetDir(folder); }}>Choose folder…</Button>
        <p className="export__path" title={targetDir}>{targetDir || "No folder selected"}</p>
        <Checkbox checked={preserve} onChange={setPreserve} label="Keep existing file layout" />
         {(kind === "model" || kind === "material") && <><h3>{kind === "model" ? "Detail levels" : "Sizes"}</h3>
        <Select value={policy} options={[{ value: "all", label: kind === "model" ? "All levels" : "All sizes" }, { value: "best", label: "Best only" }]} onChange={(v) => setPolicy(v as "all" | "best")} /></>}
        {kind === "animation" && <p className="export__hint">Exports include the animation and its rig. The preview model is left out.</p>}
        <h3>Save this export</h3><TextInput aria-label="Export setup name" placeholder="e.g. Spygame" value={name} onChange={(e) => setName(e.currentTarget.value)} />
        <Stack dir="row" gap={8}><Button disabled={!name.trim() || !targetDir || !active || !panelReady} onClick={() => void saveSetup()}>{saving ? "Saving…" : "Save setup"}</Button>
          {currentId && <Button onClick={async () => { try { await remove(currentId); choose(""); } catch (e) { setError(String(e)); } }}>Delete</Button>}</Stack>
        {pluginErrors.filter((e) => e.manifest.kind === "exporter").map((e) => <p className="export__error" key={e.manifest.id}>{e.manifest.name}: {e.error}</p>)}
      </fieldset></Pane>}>
      {!exporterId?<><PluginPicker purpose="export" plugins={available.map(e=>({id:e.manifest.id,name:e.manifest.name,description:e.manifest.description}))} onChoose={choosePlugin}
        setups={setups.map(s=>({id:s.id,name:s.name,description:`${available.find(e=>e.manifest.id===s.exporterId)?.manifest.name??"Plugin unavailable"} · ${s.targetDir}`}))} onSetup={choose}/>
        {pluginErrors.filter(e=>e.manifest.kind==="exporter").map(e=><p role="alert" className="export__error" key={e.manifest.id}>{e.manifest.name}: {e.error}</p>)}</>:<div className="export-screen__files">
        <h2 className="export-screen__lead">{current?.lastSent && fingerprints ? `${added} new and ${changed} changed since you last sent.` : selected.length ? `${selected.length} ${kind}${selected.length === 1 ? "" : "s"} ready to send.` : "Choose things to export."}</h2>
        <p className="export__hint">{current?.lastSent ? `Last sent ${new Date(current.lastSent).toLocaleString()}.` : "Review the selection and the files this plugin will write."}</p>
        <div className="export-screen__section-label">To send</div>
        {selected.map((asset) => <ExportAssetRow key={asset.id} asset={asset} status={!current?.lastAssets?.[asset.id] ? "New" : fingerprints && current.lastAssets[asset.id] !== fingerprints[asset.id] ? "Changed" : "Sent before"} />)}
        <div className="export-screen__section-label">Files to write</div>
        {!targetDir && <p className="export__hint">Choose a folder to prepare the file preview.</p>}
        {panelReady && targetDir && ids.length > 0 && !plan && <p className="export__hint">Preparing file preview…</p>}
        {plan?.error && <p className="export__error">{plan.error}</p>}
        <ul className="export-file-list">{plan?.files.map((file) => <li key={file}>{file}</li>)}</ul>
        {error && <p role="alert" className="export__error">{error}</p>}
        {report && <section><p className="export__ok">Wrote {report.written.length} files.</p>{[...report.warnings, ...report.skipped].map((warning, index) => <p key={index}>{warning}</p>)}</section>}
      </div>}
    </Workbench>
  </div>;
}

function ExportAssetRow({ asset, status }: { asset: Asset; status: string }) {
  const thumbnail = useThumbUrl(asset);
  return <div className="export-asset-row"><span className="export-asset-row__image">{thumbnail && <img src={thumbnail} alt="" />}</span><span>{asset.name}</span><small>{sourceLabel(asset)} / {asset.category}</small><span className="export-asset-row__status">{status}</span></div>;
}
