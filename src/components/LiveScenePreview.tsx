import { useEffect, useRef, useState, type ReactNode } from "react";
import { hoverScenePreview, type HoverScenePreview, type HoverPreviewSource, type HoverPreviewState } from "../services/hoverScenePreview";
import { DragSurface } from "@ldlework/toybox-sdk/ui";
import "./LiveScenePreview.css";
import type { PreviewBrightness } from "../domain/previewBrightness";

/** Controlled host surface; scene sources own their loading and playback. */
export function LiveScenePreview({active,source,fallback,draggable=false,pool=hoverScenePreview,brightness="normal"}:{active:boolean;source:HoverPreviewSource;fallback:ReactNode;draggable?:boolean;pool?:HoverScenePreview;brightness?:PreviewBrightness}) {
  const host=useRef<HTMLSpanElement>(null);
  const [state,setState]=useState<HoverPreviewState>({kind:"loading"});
  const [visible,setVisible]=useState(()=>typeof document==="undefined"||!document.hidden);
  useEffect(()=>{const update=()=>setVisible(!document.hidden);document.addEventListener("visibilitychange",update);return()=>document.removeEventListener("visibilitychange",update);},[]);
  useEffect(()=>{
    if(!active||!visible||!host.current)return;
    let alive=true;
    const stop=pool.show(host.current,source,state=>{if(alive)setState(state);});
    return()=>{alive=false;stop();};
  },[active,visible,source,pool]);
  useEffect(()=>{if(active&&visible)pool.setBrightness(host.current,brightness);},[active,visible,source,pool,brightness]);
  const content=<span className="live-scene-preview" data-preview-state={active&&visible?state.kind:"idle"} title={active&&state.kind==="error"?state.message:undefined}>
    {fallback}
    {active&&visible&&<span ref={host} className={`live-scene-preview__canvas${state.kind==="ready"?" is-ready":""}`} aria-hidden="true"/>}
    {active&&visible&&state.kind==="loading"&&<span className="live-scene-preview__status" role="status">Loading preview…</span>}
    {active&&visible&&state.kind==="error"&&<span className="live-scene-preview__status" role="status">Preview unavailable</span>}
  </span>;
  return draggable?<DragSurface onDrag={(dx,dy)=>pool.rotate(host.current,dx,dy)} onSecondaryDrag={(_dx,dy)=>pool.zoom(host.current,dy)}>{content}</DragSurface>:content;
}
