import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import ts from "typescript";
import {Group,Bone,Mesh,BoxGeometry,Box3,PerspectiveCamera,Vector3,MeshBasicMaterial,AnimationClip,VectorKeyframeTrack,QuaternionKeyframeTrack} from "three";
import { Scene, DirectionalLight, MeshStandardMaterial, Texture } from "three";
const url=source=>`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
function moduleUrl(path,replacements={}) {let {outputText}=ts.transpileModule(readFileSync(new URL(path,import.meta.url),"utf8"),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}});for(const [from,to]of Object.entries(replacements))outputText=outputText.replaceAll(`from "${from}"`,`from "${to}"`);return url(outputText);}
const three={three:import.meta.resolve("three")};
const lighting=moduleUrl("../src/services/previewLighting.ts",{...three,"../domain/previewBrightness":moduleUrl("../src/domain/previewBrightness.ts")});
const {setPreviewLighting}=await import(lighting);
test("brightness scales illumination without dimming emissives or compounding successive changes",()=>{
  const scene=new Scene(),light=new DirectionalLight(0xffffff,1.2),material=new MeshStandardMaterial({envMap:new Texture(),envMapIntensity:2,emissive:0xff0000,emissiveIntensity:2});
  scene.add(light,new Mesh(new BoxGeometry(),material));
  for(const [mode,factor] of [["dim",.3],["veryDim",.05],["normal",1]]) {
    setPreviewLighting(scene,mode);
    assert.ok(Math.abs(light.intensity-1.2*factor)<1e-8);assert.equal(scene.environmentIntensity,factor);assert.equal(material.envMapIntensity,2*factor);assert.equal(material.emissiveIntensity,2);assert.equal(material.emissive.getHex(),0xff0000);
  }
  material.envMap.dispose();material.dispose();scene.children[1].geometry.dispose();
});
const {createAnimationLoop}=await import(moduleUrl("../src/services/animationPreviewSource.ts",{
  ...three,"../domain/animationMotion":moduleUrl("../src/domain/animationMotion.ts",three),"./gltfLoad":url(`export const loadAssetScene=()=>{throw Error('unused')}`),
  "./disposeScene":url(`export const disposeScene=root=>{root.userData.disposed=true}`),"./rigGuide":moduleUrl("../src/services/rigGuide.ts",three),
}));
test("hover playback loops in place while retaining limb animation and fixed motion bounds",()=>{
  const group=new Group(),root=new Bone(),arm=new Bone();root.name="Root";arm.name="Arm";root.position.y=1;arm.position.x=1;group.add(root);root.add(arm);arm.add(new Mesh(new BoxGeometry(1,.2,.2),new MeshBasicMaterial()));
  const clip=new AnimationClip("Walk",1,[new VectorKeyframeTrack("Root.position",[0,1],[0,1,0,10,1,0]),new QuaternionKeyframeTrack("Arm.quaternion",[0,1],[0,0,0,1,0,0,Math.SQRT1_2,Math.SQRT1_2])]);
  const original=Array.from(clip.tracks[0].values),preview=createAnimationLoop(group,clip),bounds=preview.bounds.clone();
  preview.tick(.5);assert.ok(Math.abs(root.position.x)<1e-6);assert.ok(arm.quaternion.z>.1);assert.ok(preview.bounds.equals(bounds));
  const pose=arm.quaternion.toArray();preview.tick(1);assert.deepEqual(arm.quaternion.toArray(),pose,"the same pose returns after one loop");assert.ok(Math.abs(root.position.x)<1e-6);
  preview.reset();assert.ok(Math.abs(arm.quaternion.z)<1e-6);assert.deepEqual(Array.from(clip.tracks[0].values),original);preview.dispose();assert.equal(preview.subject.userData.disposed,true);
});
test("a meshless source gets a moving skeleton guide",()=>{
  const group=new Group(),root=new Bone(),child=new Bone();root.name="Root";child.name="Child";child.position.y=1;group.add(root);root.add(child);
  const preview=createAnimationLoop(group,new AnimationClip("Bend",1,[new QuaternionKeyframeTrack("Root.quaternion",[0,1],[0,0,0,1,0,0,Math.SQRT1_2,Math.SQRT1_2])]));
  assert.equal(preview.subject.children.length,2);preview.tick(.5);assert.ok(root.quaternion.z>.1);assert.ok(!preview.bounds.isEmpty());preview.dispose();
});

const {createStaticScene}=await import(moduleUrl("../src/services/staticPreviewSource.ts",{
  ...three,"./gltfLoad":url(`export const loadAssetScene=()=>{throw Error('unused')}`),
  "./materialLoad":url(`export const loadMaterial=()=>{throw Error('unused')}`),
  "./materialPreview":url(`export const buildPreviewMesh=()=>{throw Error('unused')}`),
  "./studioEnvironment":url(`export const studioEnvironment=()=>{throw Error('unused')}`),
  "./disposeScene":url(`export const disposeScene=root=>{root.userData.disposed=true}`),
}));
test("static previews retain offset geometry and authored transforms for camera orbit",()=>{
  const content=new Group(),mesh=new Mesh(new BoxGeometry(2,4,1),new MeshBasicMaterial());
  mesh.position.set(10,3,-7);content.position.set(2,1,0);content.add(mesh);
  const original=new Box3().setFromObject(content),preview=createStaticScene(content);
  assert.ok(new Box3().setFromObject(preview.subject).equals(original));
  assert.equal(preview.rotate,undefined);assert.deepEqual(content.position.toArray(),[2,1,0]);
  preview.reset();assert.ok(new Box3().setFromObject(preview.subject).equals(original));preview.dispose();
  assert.equal(preview.subject.userData.disposed,true);
});

const stages=[];
globalThis.makeHoverStage=canvas=>{const stage={renderer:{domElement:canvas,frames:0,setPixelRatio(){},setSize(){},render(){this.frames++;},dispose(){this.disposed=true;},forceContextLoss(){}},scene:{environmentIntensity:1,add(){},remove(){},traverse(){}},camera:{zoom:1,updateProjectionMatrix(){}}};stages.push(stage);return stage;};
const {HoverScenePreview}=await import(moduleUrl("../src/services/hoverScenePreview.ts",{...three,"./previewLighting":lighting,"./scenePreviewStage":url(`export const createScenePreviewStage=canvas=>globalThis.makeHoverStage(canvas)`),"./fitCamera":url(`export const fitCameraToBounds=()=>{}`)}));
const frames=new Map();let frameId=0;
globalThis.requestAnimationFrame=callback=>{frames.set(++frameId,callback);return frameId;};globalThis.cancelAnimationFrame=id=>frames.delete(id);
globalThis.window={devicePixelRatio:1};
globalThis.document={hidden:false,addEventListener(){},removeEventListener(){},createElement(){return {style:{},setAttribute(){},addEventListener(){},removeEventListener(){},remove(){this.parent=null;}};}};
globalThis.ResizeObserver=class {observe(){}disconnect(){}};
const host=()=>({isConnected:true,getBoundingClientRect:()=>({width:200,height:200}),appendChild(canvas){canvas.parent=this;}});
const live=()=>({subject:{},bounds:{},ticks:0,disposed:0,reset(){},tick(){this.ticks++;},dispose(){this.disposed++;}});
const flush=()=>new Promise(resolve=>setTimeout(resolve,5));

test("static previews prepare once and render only on rotation, coalescing drag events",async()=>{
  const pool=new HoverScenePreview({delayMs:0}),scene=live(),target=host();let prepared,rotation=0;
  scene.continuous=false;scene.prepare=({renderer})=>{prepared=renderer;};scene.rotate=dx=>{rotation+=dx;};
  pool.show(target,{key:"static",revision:[],load:async()=>scene},()=>{});await flush();
  assert.ok(prepared);assert.equal(frames.size,0);const before=prepared.frames;
  pool.rotate(host(),10,0);assert.equal(rotation,0);
  pool.rotate(target,5,0);pool.rotate(target,7,0);assert.equal(rotation,12);assert.equal(frames.size,1);
  const [id,callback]=frames.entries().next().value;frames.delete(id);callback(performance.now());
  assert.equal(prepared.frames,before+1);assert.equal(frames.size,0);pool.dispose();
});
test("obsolete hover loads are cancelled and disposed; the next card reuses one renderer",async()=>{
  const stageCount=stages.length;
  const pool=new HoverScenePreview({delayMs:0}),old=live(),current=live();let resolve,signal;
  pool.show(host(),{key:"old",revision:[],load:value=>{signal=value;return new Promise(done=>resolve=done);}},()=>{});await flush();
  const states=[];pool.show(host(),{key:"new",revision:[],load:async()=>current},state=>states.push(state));await flush();
  assert.equal(signal.aborted,true);resolve(old);await flush();assert.equal(old.disposed,1);assert.equal(states.at(-1).kind,"ready");assert.equal(frames.size,1);
  const [id,callback]=frames.entries().next().value;frames.delete(id);callback(performance.now()+100);assert.ok(current.ticks>0);
  pool.stop();assert.equal(frames.size,0);pool.dispose();assert.equal(current.disposed,1);assert.equal(stages.length,stageCount+1);
});

test("zoom is bounded and scoped to its host; inspector and hover render independently",async()=>{
  const hover=new HoverScenePreview({delayMs:0}),inspector=new HoverScenePreview({delayMs:0}),a=host(),b=host();
  const source={key:"same",revision:[],load:async()=>({...live(),continuous:false})};
  hover.show(a,source,()=>{});inspector.show(b,source,()=>{});await flush();
  const hoverStage=stages.at(-2),inspectorStage=stages.at(-1);
  hover.zoom(b,-100);assert.equal(hoverStage.camera.zoom,1);
  hover.zoom(a,-30);assert.ok(hoverStage.camera.zoom>1);assert.equal(inspectorStage.camera.zoom,1);
  inspector.zoom(b,10000);assert.equal(inspectorStage.camera.zoom,.35);
  hover.zoom(a,-10000);assert.equal(hoverStage.camera.zoom,4);
  hover.stop();assert.equal(inspectorStage.renderer.domElement.parent,b);
  hover.show(a,source,()=>{});await flush();assert.equal(hoverStage.camera.zoom,1);
  hover.dispose();inspector.dispose();assert.equal(frames.size,0);
});

test("animated preview rotation orbits the camera without transforming the motion root",async()=>{
  const pool=new HoverScenePreview({delayMs:0}),target=host(),scene=live();
  scene.subject=new Group();scene.bounds=new Box3(new Vector3(-1,0,-1),new Vector3(1,2,1));
  pool.show(target,{key:"animated",revision:[],load:async()=>scene},()=>{});await flush();
  const stage=stages.at(-1),camera=new PerspectiveCamera();stage.camera=camera;camera.position.set(3,3,3);
  const center=scene.bounds.getCenter(new Vector3()),distance=camera.position.distanceTo(center),before=camera.position.clone();
  pool.rotate(target,30,10);assert.ok(camera.position.distanceTo(before)>.1);
  assert.ok(Math.abs(camera.position.distanceTo(center)-distance)<1e-6);
  assert.deepEqual(scene.subject.position.toArray(),[0,0,0]);assert.deepEqual(scene.subject.quaternion.toArray(),[0,0,0,1]);
  // Mixed drags, full revolutions and pole limits must keep screen-right level.
  for(const [dx,dy] of [[300,50],[-900,-200],[150,10000],[-500,-10000],[40,30]]) {
    pool.rotate(target,dx,dy);
    const right=new Vector3(1,0,0).applyQuaternion(camera.quaternion);
    assert.ok(Math.abs(right.y)<1e-6,"the horizon never rolls");
    assert.ok(camera.up.equals(new Vector3(0,1,0)));
    assert.ok(Math.abs(camera.position.distanceTo(center)-distance)<1e-6);
  }
  pool.dispose();
});
test("recent scenes are reused, cache eviction is bounded and failures leave a fallback",async()=>{
  const pool=new HoverScenePreview({delayMs:0}),scenes=[live(),live(),live()];let loads=0;
  const sources=scenes.map((scene,index)=>({key:String(index),revision:["same"],load:async()=>{loads++;return scene;}}));
  const stop=pool.show(host(),sources[0],()=>{});await flush();stop();pool.show(host(),sources[0],()=>{});await flush();assert.equal(loads,1);
  pool.show(host(),sources[1],()=>{});await flush();pool.show(host(),sources[2],()=>{});await flush();pool.stop();assert.equal(scenes[0].disposed,1);
  const states=[];pool.show(host(),{key:"bad",revision:[],load:async()=>{throw Error("Broken source");}},state=>states.push(state));await flush();assert.equal(states.at(-1).kind,"error");assert.equal(frames.size,0);
  pool.dispose();assert.ok(scenes.every(scene=>scene.disposed===1));
});
