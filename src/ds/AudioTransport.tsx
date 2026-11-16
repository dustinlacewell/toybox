import { Play, Pause, RotateCcw, Square } from "lucide-react";
import { IconButton } from "./IconButton";
import { Button } from "./Button";

export function formatTime(seconds:number) { const value=Math.max(0,Math.floor(seconds||0));return `${Math.floor(value/60)}:${String(value%60).padStart(2,"0")}`; }

/** Audition intent only. Media loading and playback belong to the caller. */
export function CueButton({active=false,disabled,onCue}:{active?:boolean;disabled?:boolean;onCue:()=>void}) {
  return <IconButton icon={active?Square:Play} label={active?"Stop preview":"Play preview"} className="ds-cue-button" active={active} aria-pressed={active} disabled={disabled} onClick={onCue}/>;
}
/** Values are supplied by a media owner; controls only emit user intent. */
export function AudioTransport({playing,position,duration,volume,loop,disabled,onToggle,onSeek,onVolumeChange,onLoopChange}: {
  playing:boolean;position:number;duration:number;volume:number;loop:boolean;disabled?:boolean;
  onToggle:()=>void;onSeek:(seconds:number)=>void;onVolumeChange:(volume:number)=>void;onLoopChange:(loop:boolean)=>void;
}) {
  return <div className="ds-audio-transport">
    <IconButton icon={RotateCcw} label="Restart" onClick={()=>onSeek(0)} disabled={disabled}/>
    <Button variant="primary" onClick={onToggle} disabled={disabled}>{playing?<Pause size={15}/>:<Play size={15}/>} {playing?"Pause":"Play"}</Button>
    <span className="ds-audio-transport__time">{formatTime(position)} <span>/ {formatTime(duration)}</span></span>
    <label className="ds-audio-transport__volume">Volume<input aria-label="Volume" type="range" min="0" max="1" step="0.01" value={volume} onChange={event=>onVolumeChange(Number(event.currentTarget.value))}/></label>
    <Button variant="ghost" aria-pressed={loop} onClick={()=>onLoopChange(!loop)}>Loop</Button>
  </div>;
}
