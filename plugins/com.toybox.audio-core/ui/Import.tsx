import { useEffect, useRef, useState } from "react";
import { AudioTransport, Button, Checkbox, Pane, Stack, TextInput, Workbench } from "@ldlework/toybox-sdk/ui";
import { defineImportPanel, importNamespace, type ImportPanelCtx, type AudioSeedEntry } from "@ldlework/toybox-sdk";
import { audioId, findAudio, validSegment } from "./files.js";

export default defineImportPanel(function AudioImport({ctx}:{ctx:ImportPanelCtx}) {
  const [source,setSource]=useState<string>(),[files,setFiles]=useState<string[]>([]),[selected,setSelected]=useState(new Set<string>());
  const [pack,setPack]=useState("_import_pending"),[category,setCategory]=useState("sounds"),[busy,setBusy]=useState(false),[status,setStatus]=useState(""),[error,setError]=useState<string>();
  const [active,setActive]=useState<string>(),[preview,setPreview]=useState<string>(),[previewError,setPreviewError]=useState<string>();
  useEffect(()=>{let alive=true;let url:string|undefined;setPreview(undefined);setPreviewError(undefined);
    if(source&&active)void ctx.fs.readBytes(source,active).then(bytes=>{const ext=active.toLowerCase();url=URL.createObjectURL(new Blob([Uint8Array.from(bytes)],{type:ext.endsWith('.mp3')?'audio/mpeg':ext.endsWith('.flac')?'audio/flac':ext.endsWith('.ogg')?'audio/ogg':'audio/wav'}));if(alive)setPreview(url);else URL.revokeObjectURL(url);},error=>{if(alive)setPreviewError(String(error));});
    return()=>{alive=false;if(url)URL.revokeObjectURL(url);};
  },[source,active,ctx.fs]);
  const choose=async()=>{const folder=await ctx.fs.pickDirectory();if(!folder)return;setBusy(true);setError(undefined);setStatus("Finding audio files…");
    try{const found=await findAudio(ctx.fs,folder);setPack(importNamespace(folder));setSource(folder);setFiles(found);setSelected(new Set(found));setActive(found[0]);setStatus(found.length?`${found.length} audio files found`:"No WAV, MP3, Ogg or FLAC files found.");}
    catch(error){setError(String(error));}finally{setBusy(false);}
  };
  const run=async()=>{if(!source)return;setBusy(true);setError(undefined);
    try{const root=await ctx.host.getLibraryRoot();if(!root)throw Error("Choose a library first");const entries:AudioSeedEntry[]=[];let i=0;
      for(const file of files.filter(file=>selected.has(file))) {
        setStatus(`Copying ${++i} of ${selected.size}: ${file}`);
        if(file.split('/').some(segment=>!validSegment(segment)))throw Error(`Cannot import this path: ${file}`);
        const bytes=await ctx.fs.readBytes(source,file);const target=`library/${pack}/${category}/${file}`;
        let existing:Uint8Array|undefined;try{existing=await ctx.fs.readBytes(root,target);}catch{/* A new destination is expected. */}
        if(!existing||!(existing.length===bytes.length&&existing.every((value,index)=>value===bytes[index])))await ctx.fs.writeBytes(root,target,bytes);
        entries.push({kind:"audio",id:audioId(pack,category,file),pack,category,file,format:file.split(".").pop()?.toLowerCase()});
      }
      setStatus("Reading audio metadata and building waveforms…");await ctx.commit(entries);
    }catch(error){setError(String(error));setStatus("");}finally{setBusy(false);}
  };
  useEffect(()=>{ctx.setActions?.(<><Button disabled={busy} onClick={()=>void choose()}>{source?"Change folder":"Choose folder"}</Button><Button variant="primary" disabled={busy||!selected.size||!validSegment(pack)||!validSegment(category)} onClick={()=>void run()}>{busy?"Importing…":`Import ${selected.size||"audio"}`}</Button></>);return()=>ctx.setActions?.(null);},[ctx.setActions,source,files,selected,pack,category,busy]);
  return <Workbench navigation={<Pane><Stack gap={16} style={{padding:16}}><h3>Import into</h3>
    <label>Category<TextInput aria-label="Category" value={category} disabled={busy} onChange={event=>setCategory(event.currentTarget.value)}/></label>
    <p style={{color:"var(--text-2)",margin:0}}>WAV, MP3, Ogg Vorbis and FLAC. Storage is managed automatically by asset type.</p>
    {source&&<p style={{color:"var(--text-2)",overflowWrap:"anywhere"}}>{source}</p>}
  </Stack></Pane>} inspector={active&&<Pane><Stack gap={16} style={{padding:16}}><h3>Listen</h3><strong style={{overflowWrap:"anywhere"}}>{active.split('/').at(-1)}</strong>
    {preview&&<SourcePreview key={preview} url={preview}/>}{previewError&&<p role="alert">{previewError}</p>}
    <p style={{color:"var(--text-2)"}}>Waveform, duration, channels and sample rate are indexed when you import.</p>
  </Stack></Pane>}>
    <Stack grow style={{overflow:"hidden"}}>
      <div style={{padding:"16px 20px",borderBottom:"1px solid var(--border)"}}>{files.length>0&&<Checkbox checked={selected.size===files.length} label={`${selected.size} of ${files.length} selected`} onChange={checked=>{if(!busy)setSelected(checked?new Set(files):new Set());}}/>}<p role="status" style={{color:"var(--text-2)",marginBottom:0}}>{status||"Choose an audio folder to get started."}</p></div>
      {error&&<p role="alert" style={{padding:"0 20px",color:"var(--danger)"}}>{error}</p>}
      <div style={{overflow:"auto",flex:1}}>{files.map(file=><div key={file} style={{display:"flex",alignItems:"center",gap:12,padding:"12px 20px",borderBottom:"1px solid var(--border)",background:active===file?"var(--bg-2)":undefined}}>
        <Checkbox checked={selected.has(file)} label="" onChange={checked=>{if(!busy)setSelected(current=>{const next=new Set(current);checked?next.add(file):next.delete(file);return next;});}}/>
        <Button variant="ghost" onClick={()=>setActive(file)} style={{flex:1,justifyContent:"start",minWidth:0}}><span style={{overflow:"hidden",textOverflow:"ellipsis",whiteSpace:"nowrap"}}>{file}</span></Button><span style={{color:"var(--text-2)",fontSize:12}}>{file.split('.').at(-1)?.toUpperCase()}</span>
      </div>)}</div>
    </Stack>
  </Workbench>;
});

function SourcePreview({url}:{url:string}) {
  const ref=useRef<HTMLAudioElement>(null);
  const [playing,setPlaying]=useState(false),[position,setPosition]=useState(0),[duration,setDuration]=useState(0),[volume,setVolume]=useState(.7),[loop,setLoop]=useState(false),[error,setError]=useState<string>();
  useEffect(()=>{const element=ref.current;return()=>{element?.pause();};},[]);
  return <><audio ref={ref} src={url} loop={loop} preload="metadata" onLoadedMetadata={()=>{setDuration(ref.current?.duration??0);if(ref.current)ref.current.volume=volume;}}
    onTimeUpdate={()=>setPosition(ref.current?.currentTime??0)} onPlay={()=>setPlaying(true)} onPause={()=>setPlaying(false)} onEnded={()=>setPlaying(false)} onError={()=>setError("The browser cannot preview this file. Toybox will validate its codec when importing.")}/>
    <AudioTransport playing={playing} position={position} duration={duration} volume={volume} loop={loop} disabled={!duration||!!error}
      onToggle={()=>{const element=ref.current;if(element){if(element.paused)void element.play().catch(error=>setError(String(error)));else element.pause();}}}
      onSeek={seconds=>{if(ref.current){ref.current.currentTime=seconds;setPosition(seconds);}}} onVolumeChange={value=>{setVolume(value);if(ref.current)ref.current.volume=value;}} onLoopChange={setLoop}/>
    {error&&<p role="alert" style={{color:"var(--danger)"}}>{error}</p>}
  </>;
}
