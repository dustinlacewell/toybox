import { useEffect, useMemo, useState, useRef, type ReactNode } from "react";
import { Button, PageHeader, Select, TextInput } from "@ldlework/toybox-sdk/ui";
import type { ImportPanelCtx, ImportProvenance, SeedEntryInput } from "@ldlework/toybox-sdk";
import { withImportProvenance } from "../domain/importProvenance";
import { withOrigin } from "../domain/origin";
import type { AssetKind } from "../domain/catalog";
import { useAssetType } from "../state/assetTypes";
import { acceptsKind, importersFor, type LoadedImporter, type PluginLoadError } from "../services/pluginRegistry";
import { buildPluginBase } from "../services/pluginCtx";
import { declaresTypes, isImporter } from "../services/pluginCapabilities";
import { mergeSeedEntries, onLibraryProgress } from "../services/tauriApi";
import { unqualifyEntry } from "../domain/libraryId";
import { libraryLabel, progressLine } from "../domain/remoteLibrary";
import { syncLibrary } from "../services/libraries";
import { unwritableReason, writableLibraries } from "../domain/transfer";
import { useStore } from "../state/store";
import { stopAudioAudition } from "../services/audioAudition";
import { MaterialPreviewWidget } from "./plugin-ui/MaterialPreviewWidget";
import { PluginSlot } from "./plugin-ui/PluginSlot";
import { PluginPicker } from "./PluginPicker";
import "./ImportScreen.css";

interface Props { open: boolean; importers: LoadedImporter[]; kind: AssetKind; pluginErrors: PluginLoadError[]; onClose: () => void; onCommitted: (library: string) => void; }
export function ImportDrawer(props: Props) {
  useEffect(()=>{if(props.open)stopAudioAudition();},[props.open]);
  return props.open ? <ImportSession key={props.kind} {...props} /> : null;
}
function ImportSession({ importers: all, kind, pluginErrors, onClose, onCommitted }: Props) {
  const [selectedId, setSelectedId] = useState<string>();
  const [error, setError] = useState<string>();
  const [actions, setActions] = useState<ReactNode>(null);
  const [provenance,setProvenance]=useState<ImportProvenance>({});
  // Into a remote library the staged files are uploaded; the upload reports its progress here.
  const [committing,setCommitting]=useState(false);
  const [uploading,setUploading]=useState<string>();
  useEffect(()=>{
    let stop:(()=>void)|undefined,gone=false;
    void onLibraryProgress(progress=>{if(progress.operation==="upload"&&progress.library===libraryRef.current)setUploading(progressLine(progress));}).then(unlisten=>{if(gone)unlisten();else stop=unlisten;});
    return()=>{gone=true;stop?.();};
  },[]);
  const provenanceRef=useRef(provenance);provenanceRef.current=provenance;
  const autoPath=useRef<string|undefined>(undefined);
  // Imports go to the default library unless another is chosen; the importer
  // never knows the difference (its getLibraryRoot() is the chosen one's root).
  const libraries=useStore(state=>state.libraries);
  // Only a library that can be written to is offered; the default one first.
  const writable=writableLibraries(libraries);
  const defaultLibrary=(writable.find(library=>library.isDefault)??writable[0])?.id??"";
  const [chosenLibrary,setChosenLibrary]=useState(defaultLibrary);
  const libraryId=writable.some(library=>library.id===chosenLibrary)?chosenLibrary:defaultLibrary;
  const libraryRef=useRef(libraryId);libraryRef.current=libraryId;
  const library=libraries.find(item=>item.id===libraryId);
  const importers = importersFor(all, kind);
  const type = useAssetType(kind);
  const active = importers.find((i) => i.manifest.id === selectedId);
  const panel = active?.manifest.ui?.importPanel;
  const ctx = useMemo<ImportPanelCtx>(() => {
    const base=buildPluginBase(active?.manifest.id ?? "", active?.manifest.permissions ?? {}, ()=>libraryRef.current);
    const host=base.host;
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
    const fs=base.fs;
    fs.pickDirectory=host.pickDirectory;
    return {
      kind,
      library: library && { id: library.id, name: library.name },
      host,
      fs,
      services: base.services,
      extensions: base.extensions,
      widgets: { MaterialPreview: MaterialPreviewWidget }, setActions,
      commit: async (entries: SeedEntryInput[]) => {
        setError(undefined);
        const target=libraryRef.current;
        setUploading(undefined);setCommitting(true);
        try { await mergeSeedEntries(target, entries.map(entry=>unqualifyEntry(target,withOrigin(withImportProvenance(entry,provenanceRef.current),active?.manifest.id ?? "",active?.manifest.requiredBy ?? [])))); onCommitted(target); }
        catch (e) {
          setError(String(e).replace(/^Error: /,""));
          // A server may have taken some of the items before it refused others.
          if(libraries.find(item=>item.id===target)?.kind==="remote")void syncLibrary(target,false);
          throw e;
        }
        finally { setCommitting(false); }
      }, close: onClose,
    };
  }, [active, onClose, onCommitted, kind, library?.id, library?.name]);
  const content = useMemo(() => active && panel ? <PluginSlot key={active.manifest.id} pluginId={active.manifest.id} moduleRel={panel} ctx={ctx} fallback={null} /> : null, [active, panel, ctx]);
  return <div className="import-screen" role="dialog" aria-modal="true" aria-label="Import">
    <PageHeader title={`Import ${type.label.toLowerCase()}`} leading={<Button variant="ghost" onClick={onClose}>← Back</Button>}
      metadata={active&&<label>Import plugin <Select value={active.manifest.id} options={importers.map((i) => ({ value: i.manifest.id, label: i.manifest.name }))} onChange={(id) => { setActions(null); setError(undefined); setSelectedId(id); }} /></label>}>
      {actions}
    </PageHeader>
    {libraries.length>1&&writable.length>0&&<label className="import-screen__library">Into library <Select value={libraryId} options={writable.map(item=>({value:item.id,label:libraryLabel(item)}))} onChange={setChosenLibrary} /></label>}
    {libraries.some(item=>unwritableReason(item))&&<p className={writable.length?undefined:"import-screen__error"} style={writable.length?{color:"var(--text-2)",padding:"0 20px",margin:"0 0 8px"}:undefined} role={writable.length?"note":"alert"}>{writable.length?"Not offered: ":"Nothing can be imported: "}{libraries.filter(item=>unwritableReason(item)).map(item=>`${item.name} (${unwritableReason(item)})`).join(", ")}.</p>}
    {active&&<fieldset className="import-provenance">
      <legend>Source provenance</legend>
      {([{key:"filesystemPath",label:"Filesystem provenance",placeholder:"Original folder or archive path"},
        {key:"source",label:"Source / pack title",placeholder:"e.g. Sci-Fi Interiors"},
        {key:"publisher",label:"Publisher",placeholder:"e.g. Kenney"},
        {key:"url",label:"Source URL",placeholder:"https://…"},
        {key:"license",label:"License",placeholder:"e.g. CC0, CC BY 4.0"}] as const).map(field=><label key={field.key}>{field.label}<TextInput value={provenance[field.key]??""} placeholder={field.placeholder} onChange={event=>{const value=event.currentTarget.value;setProvenance(previous=>({...previous,[field.key]:value}));}}/></label>)}
      <p>Applies to this import. Source / pack, publisher and license become searchable tags.</p>
    </fieldset>}
    {committing&&library?.kind==="remote"&&<p className="import-screen__progress" role="status">{uploading??"Contacting the server…"}</p>}
    {error && <p className="import-screen__error" role="alert">{error}</p>}
    {pluginErrors.filter((e) => acceptsKind(e.manifest, kind) && (isImporter(e.manifest) || declaresTypes(e.manifest))).map((e) => <p key={e.manifest.id} className="import-screen__error">{e.manifest.name}: {e.error}</p>)}
    <div className="import-screen__body">{!writable.length?null:active?content??<p>This plugin has no import interface.</p>:<PluginPicker purpose="import" plugins={importers.map(i=>({id:i.manifest.id,name:i.manifest.name,description:i.manifest.description}))} onChoose={setSelectedId}/>}</div>
  </div>;
}
