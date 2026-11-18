import { useEffect, useRef, useState } from "react";
import { AudioTransport, Waveform, PropertyRow, formatTime } from "@ldlework/toybox-sdk/ui";
import type { AudioAsset } from "../domain/catalog";
import { assetUrl } from "../services/assetUrl";
import { ViewerFrame } from "./ViewerFrame";
import "./AudioViewer.css";

export function AudioViewer({asset,onClose,onExport}:{asset:AudioAsset;onClose:()=>void;onExport?:()=>void}) {
  const player=useRef<HTMLAudioElement>(null);
  const [src,setSrc]=useState<string>();const [error,setError]=useState<string>();
  const [playing,setPlaying]=useState(false),[position,setPosition]=useState(0),[volume,setVolume]=useState(.7),[loop,setLoop]=useState(false),[ready,setReady]=useState(false);
  useEffect(()=>{let active=true;const element=player.current;setSrc(undefined);setError(undefined);setReady(false);setPosition(0);setPlaying(false);
    assetUrl(asset.library,asset.audio.file).then(url=>{if(active)setSrc(url);},error=>{if(active)setError(String(error));});
    return()=>{active=false;if(element){element.pause();element.removeAttribute("src");element.load();}};
  },[asset.id,asset.audio.file]);
  const seek=(time:number)=>{if(player.current){player.current.currentTime=Math.max(0,Math.min(asset.audio.duration,time));setPosition(player.current.currentTime);}};
  const toggle=()=>{const element=player.current;if(!element)return;if(!element.paused){element.pause();return;}setError(undefined);void element.play().catch(error=>setError(String(error)));};
  return <ViewerFrame asset={asset} onClose={onClose} onExport={onExport} sidebarLabel="Audio" sidebar={<>
    <h2 className="viewer-panel-title">Audio</h2><PropertyRow label="Length" value={formatTime(asset.audio.duration)}/>
    <PropertyRow label="Format" value={asset.audio.format}/><PropertyRow label="Channels" value={asset.audio.channels===1?"Mono":asset.audio.channels===2?"Stereo":asset.audio.channels}/>
    <PropertyRow label="Sample rate" value={`${(asset.audio.sampleRate/1000).toLocaleString()} kHz`}/>
    {asset.audio.bitDepth&&<PropertyRow label="Bit depth" value={`${asset.audio.bitDepth} bit`}/>}
    <PropertyRow label="File size" value={`${(asset.audio.byteLength/1048576).toFixed(2)} MB`}/>
    <PropertyRow label="Category" value={asset.category}/>
  </>} footer={<AudioTransport playing={playing} position={position} duration={asset.audio.duration} volume={volume} loop={loop} disabled={!ready||!!error}
    onToggle={toggle} onSeek={seek} onVolumeChange={value=>{setVolume(value);if(player.current)player.current.volume=value;}} onLoopChange={setLoop}/> }>
    <audio ref={player} src={src} preload="metadata" loop={loop} onLoadedMetadata={()=>{setReady(true);if(player.current)player.current.volume=volume;}}
      onPlay={()=>setPlaying(true)} onPause={()=>setPlaying(false)} onEnded={()=>{setPlaying(false);setPosition(asset.audio.duration);}}
      onTimeUpdate={()=>setPosition(player.current?.currentTime??0)} onError={()=>{if(src)setError(`Cannot play this ${asset.audio.format} file. It can still be exported in its original format.`);}}/>
    <div className="audio-viewer"><div className="audio-viewer__heading"><span>{asset.audio.format} · {asset.audio.channels===1?"Mono":asset.audio.channels===2?"Stereo":`${asset.audio.channels} channels`}</span><h1>{asset.name}</h1><p>{playing?"Playing":ready?"Ready to play":"Loading audio…"}</p></div>
      <div className="audio-viewer__waveform"><Waveform peaks={asset.audio.peaks} duration={asset.audio.duration} position={position} onSeek={ready?seek:undefined} label="Audio timeline"/></div>
      <div className="audio-viewer__scale"><span>0:00</span><span>{formatTime(asset.audio.duration)}</span></div>
      {error&&<p role="alert" className="audio-viewer__error">{error}</p>}
    </div>
  </ViewerFrame>;
}
