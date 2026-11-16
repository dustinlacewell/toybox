import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
import {Group,Object3D,PerspectiveCamera,Vector3,AnimationClip,AnimationMixer,VectorKeyframeTrack} from 'three';
let code=ts.transpileModule(readFileSync(new URL('../src/domain/rootMotionCamera.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
code=code.replace('from "three"',`from ${JSON.stringify(import.meta.resolve('three'))}`);
const {rootMotionCamera}=await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));
const near=(a,b)=>assert.ok(a.distanceTo(b)<1e-6,`${a.toArray()} != ${b.toArray()}`);

test('camera following preserves authored root travel, jumps, orbit rotation, pan and zoom',()=>{
  const scene=new Group(),root=new Object3D();root.name='Root';scene.add(root);
  const clip=new AnimationClip('Jump',2,[new VectorKeyframeTrack('Root.position',[0,1,2],[0,0,0,4,2,3,8,0,6])]);
  const mixer=new AnimationMixer(scene);mixer.clipAction(clip).play();
  const camera=new PerspectiveCamera();camera.position.set(5,4,6);const controls={target:new Vector3(0,1,0)};
  camera.lookAt(controls.target);const follow=rootMotionCamera(camera,controls,root,new Vector3());
  const offset=camera.position.clone().sub(controls.target),rotation=camera.quaternion.clone();
  mixer.setTime(1);follow.update(true);
  near(root.position,new Vector3(4,2,3));near(camera.position.clone().sub(controls.target),offset);
  assert.ok(camera.quaternion.equals(rotation));assert.equal(camera.position.y,4,'jump remains visible');
  // Simulate an orbit, zoom and pan while the camera is following.
  controls.target.add(new Vector3(1,0,-2));camera.position.copy(controls.target).add(new Vector3(-3,2,1));camera.lookAt(controls.target);
  const userOffset=camera.position.clone().sub(controls.target),userRotation=camera.quaternion.clone();
  mixer.setTime(1.5);follow.update(true);near(camera.position.clone().sub(controls.target),userOffset);assert.ok(camera.quaternion.equals(userRotation));
  near(root.position,new Vector3(6,1,4.5));assert.equal(follow.offset.y,0);
  follow.update(false);near(camera.position,new Vector3(-2,3,-1));near(controls.target,new Vector3(1,1,-2));
  near(root.position,new Vector3(6,1,4.5));
});

test('following handles transformed roots, paused seeks, loop resets and repeated mode changes without drift',()=>{
  const parent=new Group();parent.position.set(20,2,-8);parent.scale.setScalar(2);parent.rotation.y=Math.PI/2;
  const root=new Object3D();parent.add(root);root.position.set(1,3,2);const anchor=root.getWorldPosition(new Vector3());
  const camera=new PerspectiveCamera();camera.position.set(3,5,7);const initial=camera.position.clone(),controls={target:new Vector3(1,2,3)},target=controls.target.clone();
  const follow=rootMotionCamera(camera,controls,root,anchor);
  for(const local of [new Vector3(4,5,7),new Vector3(7,3,11),new Vector3(1,3,2),new Vector3(-3,8,4)]) {
    root.position.copy(local);const world=root.getWorldPosition(new Vector3()).sub(anchor);world.y=0;
    follow.update(true);near(camera.position,initial.clone().add(world));near(controls.target,target.clone().add(world));
    follow.update(true);near(camera.position,initial.clone().add(world));
    follow.update(false);near(camera.position,initial);near(controls.target,target);near(root.position,local);
  }
});
