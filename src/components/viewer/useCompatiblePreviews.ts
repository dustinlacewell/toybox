import { useState, useMemo, useSyncExternalStore } from "react";
import { isSceneAsset, type SceneAsset } from "../../domain/catalog";
import { invalidateThumb } from "../../services/tauriApi";
import { setPreviewChoice } from "../../services/browse";
import { cachedAsset, patchCachedAsset } from "../../state/assetCache";
import { useRigCatalog } from "../../state/rigCatalog";
import { directPreviewChoices } from "../../domain/previewChoices";
import { assetMapping, referencePose, getRigProfiles, subscribeRigAdapters } from "../../services/rigAdapters";
import { mannequin, skeleton, SKELETON_ID, MANNEQUIN_ID } from "../../services/previewModel";

export function useCompatiblePreviews(asset: SceneAsset, owner: SceneAsset = asset) {
  // The rigs of the library and the models and animations bound to one: all the matching reads.
  const catalog = useRigCatalog(owner.library);
  const profiles=useSyncExternalStore(subscribeRigAdapters,getRigProfiles);
  const [savingError, setSavingError] = useState<string>();
  const remembered = owner.user.previewAssetId ?? "";
  const choices = useMemo(() => {
    if(!catalog||!asset.rigs?.length)return [];
    const byId=new Map(catalog.assets.map(candidate=>[candidate.id,candidate]));
    const direct=directPreviewChoices(asset,catalog);
    const found: {assetId:string;rigId:string;exact:boolean;asset:SceneAsset;retargeted?:boolean;builtin?:boolean}[]=direct.flatMap(choice=>{
      const candidate=byId.get(choice.assetId);
      return candidate&&isSceneAsset(candidate)?[{...choice,asset:candidate}]:[];
    });
    const mapped=assetMapping(asset,catalog);
    const foundIds=new Set(found.map(choice=>choice.assetId));
    if(mapped) {
      if(asset.kind==="animation"&&referencePose(asset,catalog))found.push({assetId:MANNEQUIN_ID,rigId:"builtin",exact:false,asset:mannequin,retargeted:true,builtin:true});
      for(const candidate of catalog.assets) {
        if(!isSceneAsset(candidate)||candidate.kind===asset.kind||foundIds.has(candidate.id))continue;
        const animation=asset.kind==="animation"?asset:candidate.kind==="animation"?candidate:undefined;
        if(animation&&referencePose(animation,catalog)&&assetMapping(candidate,catalog))found.push({assetId:candidate.id,rigId:"mapped",exact:false,asset:candidate,retargeted:true});
      }
    }
    return found.sort((a,b)=>Number(!!b.builtin)-Number(!!a.builtin)||a.asset.name.localeCompare(b.asset.name)||a.asset.pack.localeCompare(b.asset.pack));
  },[catalog?.assets,catalog?.rigs,asset,profiles]);
  const explicit=remembered===SKELETON_ID||choices.some(choice=>choice.assetId===remembered)?remembered:"";
  const defaultChoice=asset.kind==="animation"?(choices.find(choice=>choice.builtin)??choices.find(choice=>choice.assetId===asset.clip.sourceId)??choices[0]):undefined;
  const preferred=explicit||defaultChoice?.assetId||"";
  const selected=choices.find(choice=>choice.assetId===preferred);
  const target=selected?.asset??(asset.kind==="animation"?skeleton:undefined);
  const select = async (id: string) => {
    const current = cachedAsset(owner.id) ?? owner;
    const choice = id || (asset.kind==="animation"?SKELETON_ID:undefined);
    patchCachedAsset(owner.id, { user: { ...current.user, previewAssetId: choice } });
    setSavingError(undefined);
    try {
      await setPreviewChoice(owner.id, choice ?? null);
      if(owner.kind==="animation") {
        await invalidateThumb(owner.id);
        const thumb=cachedAsset(owner.id)?.thumb;
        patchCachedAsset(owner.id,{thumb:{...thumb,state:"queued",available:thumb?.state==="ready"||thumb?.available}});
      }
    }
    catch (error) { setSavingError(`Could not save preview choice: ${String(error)}`); }
  };
  return { choices, selected, target, value: selected?.assetId ?? "", select, loading: false, error: savingError };
}
