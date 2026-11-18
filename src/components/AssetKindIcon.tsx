import { useCallback, useLayoutEffect, useRef, useSyncExternalStore } from "react";
import { Box, Layers, Clapperboard, AudioLines, Shapes, type LucideIcon } from "lucide-react";
import type { AssetKind } from "../domain/catalog";

// Plugin-defined types bring their own artwork; this glyph covers the gap
// while it loads or when a plugin ships none.
const FALLBACK:Record<string,LucideIcon>={model:Box,material:Layers,animation:Clapperboard,audio:AudioLines};
type Artwork={image:HTMLImageElement;status:"loading"|"ready"|"failed";listeners:Set<()=>void>};
// Keep decoded artwork across navigation mounts. Painting it into a canvas
// before the browser paints avoids another asynchronous <img> load on each switch.
const artwork=new Map<string,Artwork>();
function subscribe(url:string|undefined,listener:()=>void) {
  if(!url)return ()=>{};
  let entry=artwork.get(url);
  if(!entry) {
    entry={image:new Image(),status:"loading",listeners:new Set()};
    artwork.set(url,entry);
    const current=entry;
    const finish=(status:Artwork["status"])=>{current.status=status;for(const notify of current.listeners)notify();};
    current.image.onload=()=>{void current.image.decode().then(()=>finish("ready"),()=>finish("failed"));};
    current.image.onerror=()=>finish("failed");
    current.image.src=url;
  }
  entry.listeners.add(listener);
  return ()=>{entry.listeners.delete(listener);};
}
export function AssetKindIcon({kind,url}:{kind:AssetKind;url?:string}) {
  const canvas=useRef<HTMLCanvasElement>(null);
  const status=useSyncExternalStore(useCallback(listener=>subscribe(url,listener),[url]),()=>url?artwork.get(url)?.status??"loading":"failed",()=>"loading");
  useLayoutEffect(()=>{
    const element=canvas.current,image=url&&artwork.get(url)?.image;
    if(status!=="ready"||!element||!image)return;
    const scale=window.devicePixelRatio||1;
    element.width=Math.round(18*scale);element.height=Math.round(18*scale);
    const ratio=Math.min(element.width/image.naturalWidth,element.height/image.naturalHeight);
    const width=image.naturalWidth*ratio,height=image.naturalHeight*ratio;
    element.getContext("2d")?.drawImage(image,(element.width-width)/2,(element.height-height)/2,width,height);
  },[url,status]);
  const Icon=FALLBACK[kind]??Shapes;
  return status==="ready"?<canvas ref={canvas} className="asset-kind-icon" aria-hidden="true"/>:<Icon size={18} aria-hidden="true"/>;
}
