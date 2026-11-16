import { CueButton, Waveform } from "@ldlework/toybox-sdk/ui";
import type { AudioAsset } from "../domain/catalog";
import { useAudioPreview } from "./useAudioPreview";
import "./AudioPreview.css";

export function AudioPreview({asset,onOpen}:{asset:AudioAsset;onOpen:()=>void}) {
  const cue=useAudioPreview(asset);
  return <div className="asset-inspector__preview audio-preview">
    <button className="audio-preview__open" aria-label="Open preview" onClick={()=>{cue.stop();onOpen();}}>
      <Waveform peaks={asset.audio.peaks}/><span className="asset-inspector__expand">↗</span>
    </button>
    <div className="audio-preview__controls"><CueButton active={cue.active} onCue={cue.toggle}/><span>{cue.playing?"Playing":cue.loading?"Loading…":""}</span></div>
    {cue.error&&<span className="audio-preview__error" role="alert">{cue.error}</span>}
  </div>;
}
