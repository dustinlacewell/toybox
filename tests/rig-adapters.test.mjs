import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { Group, Bone, AnimationClip, AnimationMixer, VectorKeyframeTrack, QuaternionKeyframeTrack, Vector3 } from "three";

function moduleUrl(file,replacements={}) {
  let {outputText}=ts.transpileModule(readFileSync(new URL(file,import.meta.url),"utf8"),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}});
  for(const [from,to]of Object.entries(replacements))outputText=outputText.replaceAll(`from "${from}"`,`from ${JSON.stringify(to)}`);
  return `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
}
const mappingUrl=moduleUrl("../src/domain/rigMapping.ts");
const {mapRig}=await import(mappingUrl);
const {retargetHumanoid,sceneMapping}=await import(moduleUrl("../src/services/humanoidRetarget.ts",{"three":import.meta.resolve("three"),"../domain/rigMapping":mappingUrl}));
const {default:plugin}=await import(moduleUrl("../plugins/com.toybox.humanoid-rigs/index.ts",{"@ldlework/toybox-sdk":import.meta.resolve("@ldlework/toybox-sdk")}));
const pluginUrl=moduleUrl("../plugins/com.toybox.humanoid-rigs/index.ts",{"@ldlework/toybox-sdk":import.meta.resolve("@ldlework/toybox-sdk")});
const registry=await import(moduleUrl("../src/services/rigAdapters.ts",{"../../plugins/com.toybox.humanoid-rigs/index":pluginUrl,"../domain/rigMapping":mappingUrl}));

function fixture(profile,scale=1,armAngle=0) {
  const scene=new Group();scene.name="Scene";const bones={};
  function add(role,parent,position) {const b=new Bone();b.name=profile.bones[role][0];b.position.fromArray(position).multiplyScalar(scale);(parent?bones[parent]:scene).add(b);bones[role]=b;}
  add("hips",null,[0,1,0]);add("spine","hips",[0,.2,0]);add("chest","spine",[0,.2,0]);add("neck","chest",[0,.2,0]);add("head","neck",[0,.2,0]);
  for(const [side,sign]of[["Left",1],["Right",-1]]) {
    add("upperArm"+side,"chest",[sign*.2,.1,0]);add("forearm"+side,"upperArm"+side,[sign*.3,0,0]);add("hand"+side,"forearm"+side,[sign*.3,0,0]);
    bones["upperArm"+side].rotation.z=sign*armAngle;
    add("upperLeg"+side,"hips",[sign*.1,0,0]);add("lowerLeg"+side,"upperLeg"+side,[0,-.45,0]);add("foot"+side,"lowerLeg"+side,[0,-.45,0]);
  }
  return {scene,bones};
}

test("bundled adapters recognize both Synty rigs and Quaternius through named topology",()=>{
  for(const profile of plugin.profiles){const {scene}=fixture(profile);assert.equal(sceneMapping(scene,plugin.profiles).mapping.profile.id,profile.id);}
});
test("duplicate bone names and wrong parent chains do not produce a confident mapping",()=>{
  const profile=plugin.profiles[0],{scene,bones}=fixture(profile);
  scene.add(bones.handLeft);assert.throws(()=>sceneMapping(scene,plugin.profiles),/unambiguous/);
  assert.equal(mapRig([{key:"a",name:"Hip",parent:null},{key:"b",name:"Hip",parent:null}],[{id:"x",name:"x",bones:{hips:["Hip"]},required:["hips"],parents:{}}]),null);
});
test("custom adapter profiles load through the contract and invalid profiles leave the registry intact",()=>{
  const before=registry.getRigProfiles();
  assert.throws(()=>registry.registerRigAdapters({profiles:[{id:"bad",name:"Bad",bones:{hips:["hip"]},required:["hips"],parents:{}}]}),/torso/);
  assert.equal(registry.getRigProfiles(),before);
  const custom=structuredClone(plugin.profiles[0]);custom.id="example.custom";custom.bones.hips=["ExternalHip"];
  let updates=0;const unsubscribe=registry.subscribeRigAdapters(()=>updates++);
  registry.registerRigAdapters({profiles:[custom]});unsubscribe();
  assert.equal(updates,1);assert.equal(sceneMapping(fixture(custom).scene,registry.getRigProfiles()).mapping.profile.id,custom.id);
});
test("profile updates invalidate cached reference-pose choices without changing the catalog",()=>{
  const profile=plugin.profiles[0],{scene}=fixture(profile),mapped=sceneMapping(scene,plugin.profiles);
  const entries=[...mapped.objects];
  const nodes=entries.map(([key,node])=>({key,name:node.name,parent:entries.find(([,candidate])=>candidate===node.parent)?.[0]??null}));
  const rigs=[{id:"rig",kind:"skeleton",unambiguous:true,nodes}];
  const source={id:"source",kind:"animation",name:"Move",rigs:[{rigId:"rig",nodes:{}}]};
  const old={...source,id:"old",name:"A_TPose_Neut"},next={...source,id:"new",name:"NewNeutral"};
  const catalog={rigs,assets:[source,old,next]};
  assert.equal(registry.referencePose(source,catalog).id,"old");
  const replacement=structuredClone(profile);replacement.referencePoseNames=["NewNeutral"];
  registry.registerRigAdapters({profiles:[replacement]});
  assert.equal(registry.referencePose(source,catalog).id,"new");
});
test("retargeting preserves target bone lengths, handles a different arm rest pose, and scales root travel",async()=>{
  const sourceProfile=plugin.profiles[0],targetProfile=plugin.profiles[2];
  const source=fixture(sourceProfile),reference=fixture(sourceProfile),target=fixture(targetProfile,2,-Math.PI/6);
  const clip=new AnimationClip("Move",1,[new VectorKeyframeTrack(`${source.bones.hips.name}.position`,[0,1],[0,1,0,1,1,0]),new QuaternionKeyframeTrack(`${source.bones.upperArmLeft.name}.quaternion`,[0,1],[0,0,0,1,0,0,-Math.SQRT1_2,Math.SQRT1_2])]);
  const sourceTrack=Array.from(clip.tracks[0].values),length=target.bones.forearmLeft.position.length();
  const result=await retargetHumanoid(source.scene,clip,reference.scene,target.scene,plugin.profiles);
  assert.deepEqual(Array.from(clip.tracks[0].values),sourceTrack,"authored samples are unchanged");
  const mixer=new AnimationMixer(target.scene);const action=mixer.clipAction(result.clip).play();action.time=.5;mixer.update(0);target.scene.updateWorldMatrix(true,true);
  assert.ok(Math.abs(target.bones.hips.getWorldPosition(new Vector3()).x-1)<1e-5,"root distance scales with proportions");
  assert.ok(Math.abs(target.bones.forearmLeft.position.length()-length)<1e-5,"limb lengths stay authored");
  const direction=target.bones.forearmLeft.getWorldPosition(new Vector3()).sub(target.bones.upperArmLeft.getWorldPosition(new Vector3())).normalize();
  assert.ok(Math.abs(direction.x-Math.SQRT1_2)<.01&&Math.abs(direction.y+Math.SQRT1_2)<.01,"arm motion survives the rest-axis correction");
  assert.ok(result.clip.tracks.every(track=>Array.from(track.values).every(Number.isFinite)));
  mixer.stopAllAction();mixer.uncacheRoot(target.scene);
});
test("thumbnail retargeting samples one pose that matches the full playback bake",async()=>{
  const source=fixture(plugin.profiles[0]),reference=fixture(plugin.profiles[0]);
  const full=fixture(plugin.profiles[2],2,-Math.PI/6),still=fixture(plugin.profiles[2],2,-Math.PI/6);
  const clip=new AnimationClip("Long motion",10,[new VectorKeyframeTrack(`${source.bones.hips.name}.position`,[0,10],[0,1,0,5,1,0])]);
  const bake=await retargetHumanoid(source.scene,clip,reference.scene,full.scene,plugin.profiles);
  const pose=await retargetHumanoid(source.scene,clip,reference.scene,still.scene,plugin.profiles,2.5);
  assert.ok(pose.clip.tracks.every(track=>track.times.length===1),"thumbnail does not bake unused frames");
  assert.ok(bake.clip.tracks.every(track=>track.times.length===301));
  const a=new AnimationMixer(full.scene),b=new AnimationMixer(still.scene);a.clipAction(bake.clip).play();b.clipAction(pose.clip).play();a.setTime(2.5);b.setTime(2.5);
  full.scene.updateWorldMatrix(true,true);still.scene.updateWorldMatrix(true,true);
  for(const role of Object.keys(full.bones)) assert.ok(full.bones[role].getWorldPosition(new Vector3()).distanceTo(still.bones[role].getWorldPosition(new Vector3()))<1e-5,role);
  a.stopAllAction();b.stopAllAction();
});

test("aborting playback baking restores the target pose and releases an obsolete job",async()=>{
  const source=fixture(plugin.profiles[0]),reference=fixture(plugin.profiles[0]),target=fixture(plugin.profiles[2],2,-Math.PI/6);
  const before=target.bones.upperArmLeft.quaternion.clone();const controller=new AbortController();
  const clip=new AnimationClip("Long motion",10,[new VectorKeyframeTrack(`${source.bones.hips.name}.position`,[0,10],[0,1,0,5,1,0])]);
  setTimeout(()=>controller.abort(),0);
  await assert.rejects(retargetHumanoid(source.scene,clip,reference.scene,target.scene,plugin.profiles,undefined,controller.signal),{name:"AbortError"});
  assert.ok(target.bones.upperArmLeft.quaternion.angleTo(before)<1e-6);
});
