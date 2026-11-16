import { AnimationClip, Matrix4, PropertyBinding, SkinnedMesh, type Group, type KeyframeTrack, type Object3D } from "three";
import { clone as cloneSkeleton } from "three/examples/jsm/utils/SkeletonUtils.js";
import type { AnimationAsset, Catalog, ModelAsset, RigBinding } from "../domain/catalog";
import { directRigMatch } from "../domain/previewChoices";
import { loadRawAssetScene, loadNeutralPose, type LoadedAsset } from "./gltfLoad";
import { disposeScene } from "./disposeScene";
import { referencePose, getRigProfiles } from "./rigAdapters";
import { retargetHumanoid } from "./humanoidRetarget";

type ModelData={loaded:LoadedAsset;objects:Object3D[];rest:{position:Object3D["position"];quaternion:Object3D["quaternion"];scale:Object3D["scale"]}[];matrices:Map<number,Matrix4>};
type PreparedClip={clip:AnimationClip;previewMethod:string};

/** One loaded model per open viewer. Changing clips keeps its mesh, textures,
 * skeleton and GPU resources; only animation data is loaded and prepared.
 */
export class ModelAnimationPreview {
  private base?:Promise<ModelData>;
  private lifetime=new AbortController();
  private cache=new Map<string,PreparedClip>();
  constructor(private model:ModelAsset,private catalog:Catalog){}

  private modelData() {
    return this.base??=loadRawAssetScene(this.model,undefined,this.lifetime.signal).then(loaded=>{
      const objects:Object3D[]=[];loaded.scene.traverse(node=>objects.push(node));
      return {loaded,objects,rest:objects.map(node=>({position:node.position.clone(),quaternion:node.quaternion.clone(),scale:node.scale.clone()})),
        matrices:new Map([...loaded.nodes??[]].map(([index,node])=>{node.updateMatrix();return [index,node.matrix.clone()];}))};
    }).catch(error=>{this.base=undefined;throw error;});
  }

  async load(animation:AnimationAsset|undefined,retarget:boolean,signal:AbortSignal):Promise<LoadedAsset> {
    const model=await this.modelData();signal.throwIfAborted();
    if(!animation)return {...model.loaded,clips:[]};
    const key=`${animation.id}:${retarget}`;
    let prepared=this.cache.get(key);
    if(!prepared){
      const source=await loadRawAssetScene(animation,undefined,signal);
      try {
        signal.throwIfAborted();
        if(retarget)prepared=await this.retarget(source,animation,model,signal);
        else prepared={clip:bindDirectClip(source,animation,model,this.model,this.catalog),previewMethod:"Same rig · direct playback"};
        signal.throwIfAborted();this.cache.set(key,prepared);
        while(this.cache.size>8)this.cache.delete(this.cache.keys().next().value!);
      }finally{disposeScene(source.scene);}
    }else {this.cache.delete(key);this.cache.set(key,prepared);}
    return {scene:model.loaded.scene,clips:[prepared.clip],nodes:model.loaded.nodes,previewMethod:prepared.previewMethod};
  }

  private async retarget(source:LoadedAsset,animation:AnimationAsset,model:ModelData,signal:AbortSignal):Promise<PreparedClip> {
    const neutral=referencePose(animation,this.catalog);
    if(!neutral)throw Error("This rig adapter needs its neutral-pose clip in the library");
    const rest=await loadNeutralPose(neutral);signal.throwIfAborted();
    // Bake on an independent skeleton so the displayed model can keep playing.
    const target=cloneSkeleton(model.loaded.scene) as Group,objects:Object3D[]=[];target.traverse(node=>objects.push(node));
    const targets=new Map<string,Object3D>();
    objects.forEach((node,index)=>{
      const pose=model.rest[index];node.position.copy(pose.position);node.quaternion.copy(pose.quaternion);node.scale.copy(pose.scale);
      targets.set(node.uuid,model.objects[index]);
    });
    try {
      const result=await retargetHumanoid(source.scene,source.clips[0],rest.scene,target,getRigProfiles(),undefined,signal);
      return {clip:rebindTracks(result.clip,name=>targets.get(name)),previewMethod:`Retargeted · ${result.sourceProfile} → ${result.targetProfile}`};
    } finally {
      // Clones share mesh resources with the live model; release only their skeletons.
      const skeletons=new Set<SkinnedMesh["skeleton"]>();
      target.traverse(node=>{if(node instanceof SkinnedMesh)skeletons.add(node.skeleton);});
      skeletons.forEach(skeleton=>skeleton.dispose());
    }
  }

  dispose() {
    this.lifetime.abort();this.cache.clear();
    void this.base?.then(model=>disposeScene(model.loaded.scene),()=>{});
  }
}

function rebindTracks(clip:AnimationClip,resolve:(name:string)=>Object3D|undefined) {
  const tracks:KeyframeTrack[]=clip.tracks.map(track=>{
    const binding=PropertyBinding.parseTrackName(track.name),node=resolve(binding.nodeName);
    if(!node||!["position","quaternion","scale"].includes(binding.propertyName))throw Error("Animation target is missing from this model");
    const rebound=track.clone();rebound.name=`${node.uuid}.${binding.propertyName}`;return rebound;
  });
  return new AnimationClip(clip.name,clip.duration,tracks,clip.blendMode);
}

/** Recheck actual rest matrices against the loaded model, then bind by stable
 * file-node identities. Similar names alone are never sufficient for playback.
 */
function bindDirectClip(source:LoadedAsset,animation:AnimationAsset,model:ModelData,asset:ModelAsset,catalog:Catalog):AnimationClip {
  let pair:{source:RigBinding;target:RigBinding}|undefined;
  for(const a of animation.rigs??[])for(const b of asset.rigs??[]) {
    if(directRigMatch({...animation,rigs:[a]},{...asset,rigs:[b]},catalog)){pair={source:a,target:b};break;}
  }
  if(!pair||!source.nodes||!model.loaded.nodes)throw Error("The animation no longer matches this model's rig");
  const keys=new Map(Object.entries(pair.source.nodes).map(([key,index])=>[index,key]));
  const registry=catalog.rigs?.find(rig=>rig.id===pair!.source.rigId);
  const parents=new Map(registry?.nodes.map(node=>[node.key,node.parent]));
  const checked=new Set<string>();
  const targets=new Map<string,Object3D>();
  for(const [index,node] of source.nodes) {
    const key=keys.get(index);if(key===undefined)continue;
    const target=model.loaded.nodes.get(pair.target.nodes[key]);if(target)targets.set(node.uuid,target);
  }
  for(const track of animation.clip.tracks??[]) {
    let key=keys.get(track.node);
    while(key!==undefined&&!checked.has(key)) {
      const node=source.nodes.get(pair.source.nodes[key]),matrix=model.matrices.get(pair.target.nodes[key]);
      node?.updateMatrix();
      if(!node||!matrix||node.matrix.elements.some((value,index)=>Math.abs(value-matrix.elements[index])>1e-5))throw Error("Rig rest frames changed; rescan the library before previewing this animation");
      checked.add(key);key=parents.get(key)??undefined;
    }
  }
  const clip=source.clips[0];if(!clip)throw Error("Animation clip is missing");
  return rebindTracks(clip,name=>{const node=PropertyBinding.findNode(source.scene,name) as Object3D|null;return node?targets.get(node.uuid):undefined;});
}
