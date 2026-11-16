import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { AnimationClip, Group } from "three";
import type { SceneAsset } from "../../domain/catalog";
import { loadAssetScene } from "../../services/gltfLoad";
import { disposeScene } from "../../services/disposeScene";
import { measureExtent, type ModelExtent } from "./modelExtent";
import { subscribeRigAdapters, getRigProfiles } from "../../services/rigAdapters";
import { useStore } from "../../state/store";
import { ModelAnimationPreview } from "../../services/modelAnimationPreview";

type ReadyScene={ kind: "ready"; group: Group; extent: ModelExtent; clips: AnimationClip[]; previewMethod?:string; loading?:boolean; previewError?:string };
export type SceneState = { kind: "loading" } | ReadyScene | { kind: "error"; message: string };
export function useScene(asset: SceneAsset, reloadNonce = 0, preview?: SceneAsset,retarget=false): SceneState {
  const adapters=useSyncExternalStore(subscribeRigAdapters,getRigProfiles);
  const root=useStore(state=>state.catalog?.libraryRoot);
  const [state, setState] = useState<SceneState>({ kind: "loading" });
  const displayed = useRef<Group | null>(null);
  const displayedBySession=useRef(false);
  const modelSession=useRef<ModelAnimationPreview|null>(null);
  const modelExtent=useRef<{group:Group;extent:ModelExtent}|null>(null);
  const revision=useRef<unknown[]>([]);
  useEffect(() => {
    let alive = true;
    const controller=new AbortController();
    const signature=[asset.id,asset.fileset,root,reloadNonce,adapters];
    const same=signature.every((value,index)=>value===revision.current[index]);
    if(!same){
      modelSession.current?.dispose();modelSession.current=null;
      revision.current=signature;modelExtent.current=null;
    }
    const catalog=useStore.getState().catalog;
    if(asset.kind==="model"&&catalog&&!modelSession.current)modelSession.current=new ModelAnimationPreview(asset,catalog);
    const session=asset.kind==="model"?modelSession.current:null;
    setState(previous=>asset.kind==="model"&&same&&previous.kind==="ready"?{...previous,loading:true,previewError:undefined}:{kind:"loading"});
    const loading=session?session.load(preview?.kind==="animation"?preview:undefined,retarget,controller.signal):loadAssetScene(asset,{preview,retarget,signal:controller.signal});
    loading.then(({ scene, clips,previewMethod }) => {
      if (!alive) { if(!session)disposeScene(scene); return; }
      const old = displayed.current,oldOwned=displayedBySession.current;
      displayed.current=scene;displayedBySession.current=!!session;
      if(!modelExtent.current||modelExtent.current.group!==scene)modelExtent.current={group:scene,extent:measureExtent(scene)};
      const next:ReadyScene={kind:"ready",group:scene,extent:modelExtent.current.extent,clips,previewMethod};
      if(old&&old!==scene&&!oldOwned)disposeScene(old);
      setState(next);
    }, (error) => { if (alive) setState(previous=>asset.kind==="model"&&same&&previous.kind==="ready"?{...previous,loading:false,previewError:String(error)}:{kind:"error",message:String(error)}); });
    return () => { alive = false;controller.abort(); };
  }, [asset.id,asset.fileset,root,reloadNonce,preview?.id,retarget,adapters]);
  useEffect(() => () => {
    modelSession.current?.dispose();if(displayed.current&&!displayedBySession.current)disposeScene(displayed.current);
    modelSession.current=null;displayed.current=null;
  }, []);
  return state;
}
