import { useEffect, useMemo, useState, useRef, type ReactNode } from "react";
import { Button, PageHeader, Select, TextInput } from "@ldlework/toybox-sdk/ui";
import type { ImportPanelCtx, ImportProvenance, SeedEntryInput } from "@ldlework/toybox-sdk";
import { withImportProvenance } from "../domain/importProvenance";
import type { AssetKind } from "../domain/catalog";
import { ASSET_KIND_LABELS } from "../domain/catalog";
import { importersFor, type LoadedImporter, type PluginLoadError } from "../services/pluginRegistry";
import { buildFsApi } from "../services/pluginHost";
import { buildSlotHost } from "../services/slotHost";
import { mergeSeedEntries } from "../services/tauriApi";
import { stopAudioAudition } from "../services/audioAudition";
import { MaterialPreviewWidget } from "./plugin-ui/MaterialPreviewWidget";
import { PluginSlot } from "./plugin-ui/PluginSlot";
import { PluginPicker } from "./PluginPicker";
import "./ImportScreen.css";

interface Props { open: boolean; importers: LoadedImporter[]; kind: AssetKind; pluginErrors: PluginLoadError[]; onClose: () => void; onCommitted: () => void; }
export function ImportDrawer(props: Props) {
  useEffect(()=>{if(props.open)stopAudioAudition();},[props.open]);
  return props.open ? <ImportSession key={props.kind} {...props} /> : null;
}
function ImportSession({ importers: all, kind, pluginErrors, onClose, onCommitted }: Props) {
  const [selectedId, setSelectedId] = useState<string>();
  const [error, setError] = useState<string>();
  const [actions, setActions] = useState<ReactNode>(null);
  const [provenance,setProvenance]=useState<ImportProvenance>({});
  const provenanceRef=useRef(provenance);provenanceRef.current=provenance;
  const autoPath=useRef<string|undefined>(undefined);
  const importers = importersFor(all, kind);
  const active = importers.find((i) => i.manifest.id === selectedId);
  const panel = active?.manifest.ui?.importPanel;
  const ctx = useMemo<ImportPanelCtx>(() => {
    const host=buildSlotHost(active?.manifest.id ?? "", active?.manifest.permissions ?? {});
    const pickDirectory=host.pickDirectory;
    host.pickDirectory=async()=>{
      const path=await pickDirectory();
      if(path){
        const previousAuto=autoPath.current;autoPath.current=path;
        const previous=provenanceRef.current;
        if(!previous.filesystemPath||previous.filesystemPath===previousAuto){
          const next={...previous,filesystemPath:path};provenanceRef.current=next;setProvenance(next);
        }
      }
      return path;
    };
    const fs=buildFsApi(active?.manifest.id ?? "",active?.manifest.permissions ?? {});
    fs.pickDirectory=host.pickDirectory;
    return {
      kind,
      host,
      fs,
      widgets: { MaterialPreview: MaterialPreviewWidget }, setActions,
      commit: async (entries: SeedEntryInput[]) => {
        setError(undefined);
        try { await mergeSeedEntries(entries.map(entry=>withImportProvenance(entry,provenanceRef.current))); onCommitted(); }
        catch (e) { setError(String(e)); throw e; }
      }, close: onClose,
    };
  }, [active, onClose, onCommitted, kind]);
  const content = useMemo(() => active && panel ? <PluginSlot key={active.manifest.id} pluginId={active.manifest.id} moduleRel={panel} ctx={ctx} fallback={null} /> : null, [active, panel, ctx]);
  return <div className="import-screen" role="dialog" aria-modal="true" aria-label="Import">
    <PageHeader title={`Import ${ASSET_KIND_LABELS[kind].toLowerCase()}`} leading={<Button variant="ghost" onClick={onClose}>← Back</Button>}
      metadata={active&&<label>Import plugin <Select value={active.manifest.id} options={importers.map((i) => ({ value: i.manifest.id, label: i.manifest.name }))} onChange={(id) => { setActions(null); setError(undefined); setSelectedId(id); }} /></label>}>
      {actions}
    </PageHeader>
    {active&&<fieldset className="import-provenance">
      <legend>Source provenance</legend>
      {([{key:"filesystemPath",label:"Filesystem provenance",placeholder:"Original folder or archive path"},
        {key:"source",label:"Source / pack title",placeholder:"e.g. Sci-Fi Interiors"},
        {key:"publisher",label:"Publisher",placeholder:"e.g. Kenney"},
        {key:"url",label:"Source URL",placeholder:"https://…"},
        {key:"license",label:"License",placeholder:"e.g. CC0, CC BY 4.0"}] as const).map(field=><label key={field.key}>{field.label}<TextInput value={provenance[field.key]??""} placeholder={field.placeholder} onChange={event=>{const value=event.currentTarget.value;setProvenance(previous=>({...previous,[field.key]:value}));}}/></label>)}
      <p>Applies to this import. Source / pack, publisher and license become searchable tags.</p>
    </fieldset>}
    {error && <p className="import-screen__error" role="alert">{error}</p>}
    {pluginErrors.filter((e) => e.manifest.kind === "importer").map((e) => <p key={e.manifest.id} className="import-screen__error">{e.manifest.name}: {e.error}</p>)}
    <div className="import-screen__body">{active?content??<p>This plugin has no import interface.</p>:<PluginPicker purpose="import" plugins={importers.map(i=>({id:i.manifest.id,name:i.manifest.name,description:i.manifest.description}))} onChoose={setSelectedId}/>}</div>
  </div>;
}
