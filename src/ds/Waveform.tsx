import { useId, useMemo } from "react";
import { waveformPath } from "./waveformPath";

/** Controlled signal display. No media loading, playback state or asset data. */
export function Waveform({ peaks, duration = 0, position = 0, onSeek, label = "Waveform" }: {
  peaks: readonly number[]; duration?: number; position?: number; onSeek?: (seconds: number) => void; label?: string;
}) {
  const id=useId().replace(/:/g, "");
  const progress=duration>0?Math.max(0,Math.min(1,position/duration)):0;
  const path=useMemo(()=>waveformPath(peaks),[peaks]);
  const seekAt=(element:SVGSVGElement,clientX:number)=>{const rect=element.getBoundingClientRect();onSeek?.(Math.max(0,Math.min(1,(clientX-rect.left)/rect.width))*duration);};
  return <svg className={`ds-waveform${onSeek ? " ds-waveform--interactive" : ""}`} viewBox="0 0 1000 200" preserveAspectRatio="none"
    role={onSeek ? "slider" : "img"} aria-label={label} tabIndex={onSeek ? 0 : undefined}
    aria-valuemin={onSeek ? 0 : undefined} aria-valuemax={onSeek ? duration : undefined} aria-valuenow={onSeek ? position : undefined}
    onClick={onSeek ? event => seekAt(event.currentTarget,event.clientX) : undefined}
    onPointerDown={onSeek ? event=>{if(event.button!==0)return;event.currentTarget.setPointerCapture(event.pointerId);event.currentTarget.focus();seekAt(event.currentTarget,event.clientX);event.preventDefault();} : undefined}
    onPointerMove={onSeek ? event=>{if(event.currentTarget.hasPointerCapture(event.pointerId))seekAt(event.currentTarget,event.clientX);} : undefined}
    onPointerUp={onSeek ? event=>{if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);} : undefined}
    onKeyDown={onSeek ? event => {
      const step=Math.min(5,duration/100);
      const next=event.key==="Home"?0:event.key==="End"?duration:event.key==="ArrowLeft"?position-step:event.key==="ArrowRight"?position+step:undefined;
      if(next!==undefined){event.preventDefault();onSeek(Math.max(0,Math.min(duration,next)));}
    } : undefined}>
    <defs><clipPath id={id}><rect width={progress*1000} height="200" /></clipPath></defs>
    <path d="M0 100H1000" stroke="currentColor" opacity=".15" />
    <path d={path} fill="currentColor" opacity=".45"/>
    <path d={path} clipPath={`url(#${id})`} fill="var(--accent)"/>
    {onSeek && <path d={`M${progress*1000} 0V200`} stroke="var(--accent)" strokeWidth="2" />}
  </svg>;
}
