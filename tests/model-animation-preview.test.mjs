import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
import {Group,Bone,AnimationClip,VectorKeyframeTrack,AnimationMixer} from 'three';
const url=code=>'data:text/javascript;base64,'+Buffer.from(code).toString('base64');
const three=import.meta.resolve('three');
const moduleUrl=(path,replacements={})=>{
 let code=ts.transpileModule(readFileSync(new URL(path,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
 for(const [from,to]of Object.entries({three,...replacements}))code=code.replaceAll(`from "${from}"`,`from "${to}"`);
 return url(code);
};
const {ModelAnimationPreview}=await import(moduleUrl('../src/services/modelAnimationPreview.ts',{
 'three/examples/jsm/utils/SkeletonUtils.js':import.meta.resolve('three/examples/jsm/utils/SkeletonUtils.js'),
 '../domain/previewChoices':moduleUrl('../src/domain/previewChoices.ts'),
 './gltfLoad':url('export const loadRawAssetScene=(...args)=>globalThis.previewFixture.load(...args);export const loadNeutralPose=async()=>globalThis.previewFixture.neutral;'),
 './disposeScene':url('export const disposeScene=scene=>{scene.userData.disposed=(scene.userData.disposed??0)+1};'),
 './rigAdapters':url('export const referencePose=()=>({id:"neutral"});export const getRigProfiles=()=>[];'),
 './humanoidRetarget':url('export const retargetHumanoid=(...args)=>globalThis.previewFixture.retarget(...args);'),
}));
function fixture(){
 const scene=new Group(),root=new Bone();root.name='Root';scene.add(root);root.updateMatrix();
 const model={id:'model',kind:'model',fileset:{gltf:'body.gltf'},rigs:[{rigId:'rig',nodes:{Root:0}}]};
 const animation=id=>({id,kind:'animation',fileset:{gltf:id+'.gltf'},rigs:model.rigs,clip:{index:0,tracks:[{node:0,path:'translation'}]}});
 const catalog={assets:[],rigs:[{id:'rig',kind:'skeleton',unambiguous:true,nodes:[{key:'Root',parent:null,rest:root.matrix.toArray()}]}]};
 const sources=[],reads=[];
 const data={model,scene,root,catalog,animation,sources,reads,neutral:{scene:new Group(),clips:[]},
  load:async asset=>{
   reads.push(asset.id);if(asset.kind==='model')return {scene,clips:[],nodes:new Map([[0,root]])};
   const source=new Group(),bone=new Bone();bone.name='Root';source.add(bone);bone.updateMatrix();sources.push(source);
   return {scene:source,nodes:new Map([[0,bone]]),clips:[new AnimationClip(asset.id,1,[new VectorKeyframeTrack('Root.position',[0,1],[0,0,0,2,0,0])])]};
  },retarget:async()=>{throw Error('unused')},
 };
 globalThis.previewFixture=data;return data;
}
test('animation choices reuse one model, bind clips to its existing bones and cache animation data',async()=>{
 const f=fixture(),session=new ModelAnimationPreview(f.model,f.catalog),signal=new AbortController().signal;
 const initial=await session.load(undefined,false,signal);assert.equal(initial.scene,f.scene);
 const first=await session.load(f.animation('walk'),false,signal);
 assert.equal(first.scene,initial.scene);assert.ok(first.clips[0].tracks[0].name.startsWith(f.root.uuid));
 const mixer=new AnimationMixer(first.scene);mixer.clipAction(first.clips[0]).play();mixer.setTime(.5);assert.equal(f.root.position.x,1);
 // A live pose must not be mistaken for the model's rest pose during matching.
 const second=await session.load(f.animation('run'),false,signal);assert.equal(second.scene,initial.scene);assert.equal(f.root.position.x,1);
 const repeated=await session.load(f.animation('walk'),false,signal);assert.equal(repeated.clips[0],first.clips[0]);
 assert.deepEqual(f.reads,['model','walk','run']);assert.ok(f.sources.every(source=>source.userData.disposed===1));
 mixer.stopAllAction();mixer.uncacheRoot(first.scene);session.dispose();await Promise.resolve();assert.equal(f.scene.userData.disposed,1);
});
test('retargeting uses a temporary rest-pose skeleton and rebinds its result without touching the live model',async()=>{
 const f=fixture(),session=new ModelAnimationPreview(f.model,f.catalog),signal=new AbortController().signal;
 await session.load(undefined,false,signal);f.root.position.set(5,0,0);
 f.retarget=async(_source,_clip,_neutral,target)=>{
  const bone=target.getObjectByName('Root');assert.notEqual(bone,f.root);assert.equal(bone.position.x,0);
  bone.position.x=20;
  return {clip:new AnimationClip('mapped',1,[new VectorKeyframeTrack(bone.uuid+'.position',[0,1],[0,0,0,3,0,0])]),sourceProfile:'source',targetProfile:'target'};
 };
 const result=await session.load(f.animation('mapped'),true,signal);
 assert.equal(result.scene,f.scene);assert.equal(f.root.position.x,5);assert.ok(result.clips[0].tracks[0].name.startsWith(f.root.uuid));
 assert.deepEqual(f.reads,['model','mapped']);session.dispose();
});
