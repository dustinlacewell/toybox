import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { Group, Object3D, AnimationClip, VectorKeyframeTrack, QuaternionKeyframeTrack } from "three";

const source = readFileSync(new URL("../src/domain/animationMotion.ts", import.meta.url), "utf8");
let { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
outputText = outputText.replace('from "three"', `from ${JSON.stringify(import.meta.resolve("three"))}`);
const { rootMotion } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);

test("root-motion measurements choose the shallowest target and account for parent scale", () => {
  const scene = new Group(); scene.scale.set(2, 2, 2);
  const root = new Object3D(); root.name = "Root";
  const hand = new Object3D(); hand.name = "Hand"; root.add(hand); scene.add(root);
  const clip = new AnimationClip("Walk", 2, [
    new VectorKeyframeTrack("Hand.position", [0, 2], [0, 0, 0, 99, 0, 0]),
    new VectorKeyframeTrack("Root.position", [0, 2], [0, 0, 0, 0, 0, 3]),
  ]);
  const motion = rootMotion(scene, clip);
  assert.equal(motion.node, root);
  assert.deepEqual(motion.displacement.toArray(), [0, 0, 6]);
  assert.deepEqual(motion.start.toArray(), [0, 0, 0]);
});

test("a rotation-only clip has no invented root translation", () => {
  const scene = new Group(); const root = new Object3D(); root.name = "Root"; scene.add(root);
  const clip = new AnimationClip("Turn", 2, [new QuaternionKeyframeTrack("Root.quaternion", [0, 2], [0, 0, 0, 1, 0, 1, 0, 0])]);
  assert.equal(rootMotion(scene, clip), null);
});

test("in-place playback preserves world vertical motion and restores travel while paused", () => {
  const scene = new Group();
  const frame = new Object3D(); frame.rotation.x = Math.PI / 2; scene.add(frame);
  const root = new Object3D(); root.name = "Root"; frame.add(root);
  const clip = new AnimationClip("Jump",2,[new VectorKeyframeTrack("Root.position",[0,2],[0,0,0,4,6,2])]);
  const motion = rootMotion(scene,clip);
  motion.apply(1,true); scene.updateWorldMatrix(true,true);
  const world = root.getWorldPosition(new (root.position.constructor)());
  assert.ok(Math.abs(world.x)<1e-6); assert.ok(Math.abs(world.z)<1e-6);
  assert.ok(Math.abs(world.y+1)<1e-6);
  motion.apply(1,false);
  assert.deepEqual(root.position.toArray(),[2,3,1]);
});
