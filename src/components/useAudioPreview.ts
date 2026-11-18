import { useSyncExternalStore } from "react";
import type { AudioAsset } from "../domain/catalog";
import { getAudioAudition, stopAudioAudition, subscribeAudioAudition, toggleAudioAudition } from "../services/audioAudition";

/** Controlled views of the single library audition player. */
export function useAudioPreview(asset:AudioAsset) {
  const state=useSyncExternalStore(subscribeAudioAudition,getAudioAudition);
  const owns=state.assetId===asset.id;
  return {playing:owns&&state.status==="playing",loading:owns&&state.status==="loading",active:owns&&state.status!=="error",error:owns?state.error:undefined,
    toggle:()=>{void toggleAudioAudition(asset.id,asset.library,asset.audio.file);},stop:stopAudioAudition};
}
