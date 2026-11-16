import { assetUrl } from "./assetUrl";

type AuditionState = { assetId: string | null; status: "idle" | "loading" | "playing" | "error"; error?: string };
let state:AuditionState={assetId:null,status:"idle"};
let player:HTMLAudioElement|null=null;
const listeners=new Set<()=>void>();
export const getAudioAudition=()=>state;
export const subscribeAudioAudition=(listener:()=>void)=>{listeners.add(listener);return()=>{listeners.delete(listener);};};
function publish(value:AuditionState) {state=value;listeners.forEach(listener=>listener());}

/** One lazy player is shared by tiles and the inspector. No per-tile preload. */
export function stopAudioAudition() {
  const previous=player;player=null;
  if(previous){previous.pause();previous.removeAttribute("src");previous.load();}
  publish({assetId:null,status:"idle"});
}

export async function toggleAudioAudition(assetId:string,file:string) {
  if(state.assetId===assetId&&state.status!=="error"){stopAudioAudition();return;}
  stopAudioAudition();
  const current=new Audio();player=current;current.preload="none";current.volume=.7;
  const fail=(error:string)=>{if(player!==current)return;stopAudioAudition();publish({assetId,status:"error",error});};
  current.onerror=()=>fail("Cannot preview this audio file.");
  current.onended=()=>{if(player===current)stopAudioAudition();};
  publish({assetId,status:"loading"});
  try {
    const url=await assetUrl(file);
    // A different cue or a stop can supersede a pending native path lookup.
    if(player!==current)return;
    current.src=url;
    await current.play();
    if(player===current)publish({assetId,status:"playing"});
  } catch(error) {fail(String(error));}
}
