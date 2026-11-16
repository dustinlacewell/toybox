import { useCallback, useState } from "react";
import { Button, Checkbox, Dialog, NavigationItem, Select, TextInput, Workbench } from "@ldlework/toybox-sdk/ui";
import { Plus } from "lucide-react";
import { ASSET_KIND_LABELS, type AssetKind } from "../domain/catalog";
import type { SavedExport } from "../domain/savedExport";
import type { LoadedExporter, PluginLoadError } from "../services/pluginRegistry";
import { pickDirectory } from "../services/pickDirectory";
import { useSavedExports } from "../state/savedExports";
import { useStore } from "../state/store";
import { ExportPanelHost } from "./plugin-ui/ExportPanelHost";
import "./ExportsModal.css";

interface Props {
  open: boolean;
  kind: AssetKind;
  selectedCount: number;
  exporters: LoadedExporter[];
  pluginErrors: PluginLoadError[];
  onClose: () => void;
  onExport: (setup: SavedExport) => void;
}

export function ExportsModal(props: Props) {
  return props.open ? <ExportsManager {...props}/> : null;
}

function ExportsManager({ kind, selectedCount, exporters, pluginErrors, onClose, onExport }: Props) {
  const libraryRoot = useStore(state => state.catalog?.libraryRoot ?? "");
  const saved = useSavedExports(state => state.exports);
  const setups = saved.filter(setup => setup.libraryRoot === libraryRoot).sort((a,b) => a.name.localeCompare(b.name));
  const [selected, setSelected] = useState<string | undefined>(() => setups.find(setup => setup.kind === kind)?.id ?? setups[0]?.id);
  const [newVersion, setNewVersion] = useState(0);
  const [busy, setBusy] = useState(false);
  const current = setups.find(setup => setup.id === selected);
  return <Dialog open title="Exports" size="medium" onClose={() => { if (!busy) onClose(); }}>
    <Workbench className="exports-manager" navigation={<>
      <Button className="exports-manager__new" disabled={busy} onClick={() => { setSelected(undefined); setNewVersion(version => version+1); }}><Plus size={16} aria-hidden="true"/>New export</Button>
      <nav aria-label="Saved exports">{setups.map(setup => <NavigationItem key={setup.id} active={setup.id === selected} disabled={busy} onPress={() => setSelected(setup.id)}
        label={<span className="exports-manager__setup"><span title={setup.name}>{setup.name}</span><small>{ASSET_KIND_LABELS[setup.kind]}</small></span>}/>)}</nav>
      {!setups.length && <p className="exports-manager__hint">Save a plugin, folder, and settings to reuse for future exports.</p>}
    </>}>
      <ExportSetupEditor key={current?.id ?? `new:${newVersion}`} initial={current} kind={kind} libraryRoot={libraryRoot} exporters={exporters} pluginErrors={pluginErrors}
        selectedCount={selectedCount} onBusy={setBusy} onSaved={setup => setSelected(setup.id)}
        onRemoved={id => setSelected(setups.find(setup => setup.id !== id)?.id)} onExport={onExport}/>
    </Workbench>
  </Dialog>;
}

interface EditorProps {
  initial?: SavedExport;
  kind: AssetKind;
  libraryRoot: string;
  exporters: LoadedExporter[];
  pluginErrors: PluginLoadError[];
  selectedCount: number;
  onBusy: (busy: boolean) => void;
  onSaved: (setup: SavedExport) => void;
  onRemoved: (id: string) => void;
  onExport: (setup: SavedExport) => void;
}

function ExportSetupEditor({ initial, kind: selectionKind, libraryRoot, exporters, pluginErrors, selectedCount, onBusy, onSaved, onRemoved, onExport }: EditorProps) {
  const [kind, setKind] = useState(initial?.kind ?? selectionKind);
  const [name, setName] = useState(initial?.name ?? "");
  const [exporterId, setExporterId] = useState(initial?.exporterId ?? "");
  const [folder, setFolder] = useState(initial?.targetDir ?? "");
  const [preserve, setPreserve] = useState(initial?.preserveStructure ?? true);
  const [policy, setPolicy] = useState<"all" | "best">(initial?.variantPolicy ?? "all");
  const [config, setConfig] = useState(initial?.config ?? {});
  const [initialConfig, setInitialConfig] = useState(initial?.config);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  const available = exporters.filter(plugin => (plugin.manifest.assetKinds ?? ["model"]).includes(kind));
  const active = available.find(plugin => plugin.manifest.id === exporterId);
  const valid = !!name.trim() && !!folder && !!active && ready;
  const onConfig = useCallback((values: Record<string, unknown>, valid: boolean) => {
    setConfig(previous => JSON.stringify(previous) === JSON.stringify(values) ? previous : values);
    setReady(valid);
    setSaved(false);
  }, []);
  const choosePlugin = (id: string) => { setExporterId(id); setInitialConfig(undefined); setConfig({}); setReady(false); setSaved(false); };
  const save = async (send = false) => {
    if (!valid || busy) return;
    setBusy(true); onBusy(true); setError(undefined);
    try {
      const reconfigured = initial && (initial.exporterId !== exporterId || initial.targetDir !== folder || initial.preserveStructure !== preserve || initial.variantPolicy !== policy || JSON.stringify(initial.config) !== JSON.stringify(config));
      const setup: SavedExport = { ...initial, id: initial?.id ?? crypto.randomUUID(), name: name.trim(), kind, libraryRoot, exporterId, targetDir: folder, preserveStructure: preserve, variantPolicy: policy, config,
        lastSent: reconfigured ? undefined : initial?.lastSent, lastAssets: reconfigured ? undefined : initial?.lastAssets };
      await useSavedExports.getState().save(setup);
      setSaved(true); onSaved(setup);
      if (send) onExport(setup);
    } catch (failure) { setError(String(failure)); }
    finally { setBusy(false); onBusy(false); }
  };
  const remove = async () => {
    if (!initial || busy) return;
    setBusy(true); onBusy(true); setError(undefined);
    try { await useSavedExports.getState().remove(initial.id); onRemoved(initial.id); }
    catch (failure) { setError(String(failure)); }
    finally { setBusy(false); onBusy(false); }
  };
  return <div className="exports-manager__editor">
    <div className="exports-manager__fields" onChange={() => setSaved(false)}>
      <h2>{initial ? initial.name : "New export"}</h2>
      {initial?.lastSent && <p className="exports-manager__hint">Last sent {new Date(initial.lastSent).toLocaleString()}.</p>}
      <fieldset disabled={busy}>
        <label className="exports-manager__field">Name<TextInput placeholder="e.g. Spygame" value={name} onChange={event => setName(event.currentTarget.value)}/></label>
        <label className="exports-manager__field">Type<Select value={kind} disabled={!!initial} options={Object.entries(ASSET_KIND_LABELS).map(([value,label]) => ({value,label}))} onChange={value => { setKind(value as AssetKind); choosePlugin(""); }}/></label>
        <label className="exports-manager__field">Export plugin<Select value={exporterId} options={[{value:"",label:"Choose a plugin…"},...(!active && exporterId ? [{value:exporterId,label:"Plugin unavailable"}] : []),...available.map(plugin => ({value:plugin.manifest.id,label:plugin.manifest.name}))]} onChange={choosePlugin}/></label>
        {!available.length && <p className="exports-manager__hint">No export plugins support this type.</p>}
        {active && <ExportPanelHost key={exporterId} plugin={active} initialConfig={initialConfig} shared={{targetDir:folder,preserveStructure:preserve}} onConfig={onConfig}/>}
        <div className="exports-manager__field">Destination folder<div className="exports-manager__folder"><TextInput aria-label="Destination folder" readOnly value={folder} placeholder="Choose a folder…" title={folder}/><Button onClick={() => void (async () => {
          try { const path=await pickDirectory(); if(path){setFolder(path);setSaved(false);} }
          catch(failure){setError(String(failure));}
        })()}>Browse…</Button></div></div>
        <Checkbox checked={preserve} onChange={setPreserve} label="Keep existing file layout"/>
        {(kind === "model" || kind === "material") && <label className="exports-manager__field">{kind === "model" ? "Detail levels" : "Sizes"}<Select value={policy} options={[{value:"all",label:kind === "model" ? "All levels" : "All sizes"},{value:"best",label:"Best only"}]} onChange={value => setPolicy(value as "all" | "best")}/></label>}
        {kind === "animation" && <p className="exports-manager__hint">Exports include the animation and its rig.</p>}
        {pluginErrors.filter(failure => failure.manifest.kind === "exporter").map(failure => <p className="exports-manager__error" key={failure.manifest.id}>{failure.manifest.name}: {failure.error}</p>)}
      </fieldset>
      {error && <p role="alert" className="exports-manager__error">{error}</p>}
    </div>
    <footer className="exports-manager__footer">
      {initial && <Button variant="danger" disabled={busy} onClick={() => void remove()}>Delete setup</Button>}
      <span className="exports-manager__status" role="status">{saved ? "Saved" : ""}</span>
      {selectedCount > 0 && kind === selectionKind && <Button disabled={!valid || busy} onClick={() => void save(true)}>Export selection ({selectedCount})</Button>}
      <Button variant="primary" disabled={!valid || busy} onClick={() => void save()}>{busy ? "Saving…" : "Save"}</Button>
    </footer>
  </div>;
}
