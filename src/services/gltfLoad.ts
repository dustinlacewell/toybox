/** Load the same normalized glTF used by native exporters. Resource references
 * resolve by full library path so equal atlas basenames cannot collide. */
import { AnimationMixer, type AnimationClip, Group, LoadingManager, type Object3D } from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { TGALoader } from "three/examples/jsm/loaders/TGALoader.js";
import { loadRigCatalog } from "../state/rigCatalog";
import { clone as cloneSkeleton } from "three/examples/jsm/utils/SkeletonUtils.js";
import type { SceneAsset } from "../domain/catalog";
import { gltfResourcePath } from "../domain/gltfResource";
import { assetUrl } from "./assetUrl";
import { readPreviewSource, readPreviewPair } from "./tauriApi";
import { directPreviewChoices, directRigMatch } from "../domain/previewChoices";
import { MANNEQUIN_ID, SKELETON_ID } from "./previewModel";
import { retargetHumanoid } from "./humanoidRetarget";
import { assetMapping, getRigProfiles, referencePose } from "./rigAdapters";
import { disposeScene, shareSceneResources } from "./disposeScene";

let mannequinTemplate:Promise<Group>|undefined;
const neutralPoses=new WeakMap<object,Promise<LoadedAsset>>();
async function loadMannequin():Promise<LoadedAsset> {
  mannequinTemplate??=new GLTFLoader().loadAsync(`${import.meta.env.BASE_URL}preview/mannequin.glb`).then(gltf=>{shareSceneResources(gltf.scene);return gltf.scene;}).catch(error=>{mannequinTemplate=undefined;throw error;});
  return {scene:cloneSkeleton(await mannequinTemplate) as Group,clips:[]};
}
export async function loadNeutralPose(asset:SceneAsset):Promise<LoadedAsset> {
  const key=asset.rigs??asset;
  let cached=neutralPoses.get(key);
  if(!cached){cached=loadRawAssetScene(asset).then(result=>{
    if(result.clips[0]){const mixer=new AnimationMixer(result.scene);mixer.clipAction(result.clips[0]).play();mixer.update(0);}
    return result;
  }).catch(error=>{neutralPoses.delete(key);throw error;});neutralPoses.set(key,cached);}
  return cached;
}

export interface LoadedAsset {
  /** Original file node indices, used to bind clips without reparsing a model. */
  nodes?:Map<number,Object3D>;
  previewMethod?: string;
  scene: Group;
  clips: AnimationClip[];
}

export interface SceneLoadOptions {
  preview?: SceneAsset;
  retarget?: boolean;
  /** A thumbnail needs one pose; playback leaves this unset. */
  sampleTime?: number;
  /** Cancels obsolete loads and retarget baking, releasing owned resources. */
  signal?: AbortSignal;
}

/** Each call reads fresh JSON, including origin edits and conversion repairs.
 * The caller owns disposal of the returned scene. */
export async function loadAssetScene(asset: SceneAsset, options:SceneLoadOptions={}): Promise<LoadedAsset> {
  const {preview,retarget=false,sampleTime,signal}=options;
  signal?.throwIfAborted();
  // The rigs of the asset's library and the models and animations bound to one.
  const catalog=await loadRigCatalog(asset.library);
  if(asset.kind==="animation"&&!preview&&asset.user.previewAssetId) {
    const id=asset.user.previewAssetId;
    if(id===SKELETON_ID)return loadRawAssetScene(asset,undefined,signal);
    if(id!==MANNEQUIN_ID&&catalog) {
      const chosen=catalog.assets.find((candidate): candidate is import("../domain/catalog").ModelAsset=>candidate.kind==="model"&&candidate.id===id);
      if(chosen)return loadAssetScene(asset,{...options,preview:chosen,retarget:!directRigMatch(asset,chosen,catalog)&&!!(assetMapping(asset,catalog)&&assetMapping(chosen,catalog)&&referencePose(asset,catalog))});
    }
  }
  const auto=asset.kind==="animation"&&!preview&&catalog&&assetMapping(asset,catalog)&&referencePose(asset,catalog);
  if(preview?.id===SKELETON_ID)return loadRawAssetScene(asset,undefined,signal);
  if(auto||preview?.id===MANNEQUIN_ID||retarget) {
    if(!catalog)throw Error("Load the library before retargeting");
    const animation=asset.kind==="animation"?asset:preview?.kind==="animation"?preview:undefined;
    const model=asset.kind==="model"?asset:preview?.kind==="model"&&preview.id!==MANNEQUIN_ID?preview:undefined;
    if(!animation)throw Error("Choose an animation to retarget");
    const neutral=referencePose(animation,catalog);
    if(!neutral)throw Error("This rig adapter needs its neutral-pose clip in the library");
    const owned:Group[]=[];
    try {
      const source=await loadRawAssetScene(animation,undefined,signal);owned.push(source.scene);
      const rest=await loadNeutralPose(neutral);
      signal?.throwIfAborted();
      const target=model?await loadRawAssetScene(model,undefined,signal):await loadMannequin();
      owned.push(target.scene);
      signal?.throwIfAborted();
      const result=await retargetHumanoid(source.scene,source.clips[0],rest.scene,target.scene,getRigProfiles(),sampleTime,signal);
      owned.splice(owned.indexOf(target.scene),1);
      return {scene:target.scene,clips:[result.clip],previewMethod:`Retargeted · ${result.sourceProfile} → ${result.targetProfile}`};
    } finally {owned.forEach(disposeScene);}
  }
  if(asset.kind==="animation"&&!preview&&catalog) {
    const choices=directPreviewChoices(asset,catalog);
    const choice=choices.find(choice=>choice.assetId===asset.clip.sourceId)??choices[0];
    const candidate=choice&&catalog.assets.find((candidate): candidate is import("../domain/catalog").ModelAsset=>candidate.kind==="model"&&candidate.id===choice.assetId);
    if(candidate)return loadRawAssetScene(asset,candidate,signal);
  }
  return loadRawAssetScene(asset,preview,signal);
}

export async function loadRawAssetScene(asset:SceneAsset,preview?:SceneAsset,signal?:AbortSignal):Promise<LoadedAsset> {
  signal?.throwIfAborted();
  if (preview && preview.kind === asset.kind) throw new Error("Choose a model for an animation, or an animation for a model.");
  const animation = asset.kind === "animation" ? asset : preview?.kind === "animation" ? preview : undefined;
  const model = asset.kind === "model" ? asset : preview?.kind === "model" ? preview : undefined;
  if (preview && preview.library && preview.library !== asset.library) throw new Error("A preview must come from the same library as the item it is shown with.");
  const library = asset.library;
  const resources = [asset, ...(preview ? [preview] : [])].flatMap(asset => [asset.fileset.bin, ...asset.fileset.textures]);
  const [doc, entries] = await Promise.all([
    preview && animation && model ? readPreviewPair(library,animation.fileset.gltf,animation.clip.index,model.fileset.gltf) : readPreviewSource(library,asset.fileset.gltf,asset.kind==="animation"?asset.clip.index:null),
    Promise.all(resources.map(async rel =>
      [rel.replace(/\\/g, "/"), await assetUrl(library, rel)] as const)),
  ]);
  const urls = new Map(entries);
  signal?.throwIfAborted();
  const manager = new LoadingManager();
  manager.addHandler(/\.tga$/i, new TGALoader(manager));
  manager.setURLModifier(uri => {
    if (uri.startsWith("toybox-resource:")) {
      const url = urls.get(uri.slice("toybox-resource:".length));
      if (!url) throw new Error("A preview resource is missing from its fileset.");
      return url;
    }
    const path = gltfResourcePath(asset.fileset.gltf, uri);
    return path === null ? uri : urls.get(path) ?? uri;
  });
  const gltf = await new GLTFLoader(manager).parseAsync(JSON.stringify(doc), "");
  if(signal?.aborted){disposeScene(gltf.scene);signal.throwIfAborted();}
  // The native preview read already selects the original clip by source index.
  const clips = realClips(gltf.animations);
  if (asset.kind === "animation" && !clips.length) {disposeScene(gltf.scene);throw new Error("The selected animation clip is missing from its source.");}
  const nodes=new Map<number,Object3D>();
  gltf.scene.traverse(node=>{const index=gltf.parser.associations.get(node)?.nodes;if(index!==undefined)nodes.set(index,node);});
  return { scene: gltf.scene, clips, nodes };
}

function realClips(animations: AnimationClip[] | undefined): AnimationClip[] {
  return (animations ?? []).filter(clip => clip.tracks.length > 0);
}
