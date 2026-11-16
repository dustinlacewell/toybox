import { AnimationMixer, Box3, Group, LoopRepeat, SkinnedMesh, Vector3 } from "three";
import type { AnimationAsset } from "../domain/catalog";
import { rootMotion } from "../domain/animationMotion";
import { loadAssetScene } from "./gltfLoad";
import { disposeScene } from "./disposeScene";
import { createRigGuide, hasVisibleGeometry } from "./rigGuide";
import type { HoverPreviewSource, LiveScene } from "./hoverScenePreview";

/** Adapts catalog animation facts to the generic live-scene preview contract. */
export function animationPreviewSource(asset:AnimationAsset,revision:readonly unknown[]):HoverPreviewSource {
  return {key:asset.id,revision,load:async signal=>{
    const loaded=await loadAssetScene(asset,{signal});
    try {signal.throwIfAborted();return createAnimationLoop(loaded.scene,loaded.clips[0]);}
    catch(error){disposeScene(loaded.scene);throw error;}
  }};
}

export function createAnimationLoop(group:Group,clip:import("three").AnimationClip):LiveScene {
  if(!clip?.tracks.length)throw Error("No playable animation clip");
  const subject=new Group();subject.add(group);
  const motion=rootMotion(group,clip),mixer=new AnimationMixer(group),action=mixer.clipAction(clip);
  action.setLoop(LoopRepeat,Infinity).play();
  const hasMesh=hasVisibleGeometry(group);
  const bounds=new Box3(),point=new Vector3();
  // Sample motion bounds, refreshing skinned bounds at each pose. Horizontal
  // root travel is clamped; jumps and limb motion retain their authored height.
  action.paused=true;
  for(let i=0;i<24;i++) {
    action.time=Math.max(0,clip.duration)*i/24;mixer.update(0);motion?.apply(action.time,true);group.updateWorldMatrix(true,true);
    if(hasMesh){group.traverse(node=>{if(node instanceof SkinnedMesh)node.computeBoundingBox();});bounds.union(new Box3().setFromObject(group));}
    else group.traverse(node=>bounds.expandByPoint(node.getWorldPosition(point)));
  }
  if(bounds.isEmpty())bounds.setFromCenterAndSize(new Vector3(),new Vector3(1,1,1));
  const guide=hasMesh?undefined:createRigGuide(group,Math.max(bounds.getSize(new Vector3()).length(),.01));
  if(guide)subject.add(guide.object);
  const reset=()=>{action.reset().setLoop(LoopRepeat,Infinity).play();mixer.update(0);motion?.apply(0,true);guide?.update();};
  reset();
  return {subject,bounds,reset,tick:delta=>{mixer.update(delta);motion?.apply(action.time,true);group.updateWorldMatrix(true,true);guide?.update();},
    dispose:()=>{mixer.stopAllAction();mixer.uncacheRoot(group);disposeScene(subject);}};
}
