import { Matrix3, PropertyBinding, Vector3, type AnimationClip, type Group, type Object3D, type Interpolant, type KeyframeTrack } from "three";

type TrackWithInterpolant = KeyframeTrack & { createInterpolant: ((result?: Float32Array) => Interpolant) & { isInterpolantFactoryMethodGLTFCubicSpline?: boolean } };

/** Use the shallowest animated translation node, never an arbitrary hand/foot. */
export function rootMotion(scene: Group, clip: AnimationClip) {
  scene.updateWorldMatrix(true, true);
  const tracks = (clip.tracks as TrackWithInterpolant[]).filter(track => track.name.endsWith(".position") &&
    (track.getValueSize() === 3 || (track.getValueSize() === 9 && track.createInterpolant.isInterpolantFactoryMethodGLTFCubicSpline)));
  const candidates = tracks.flatMap((track) => {
    const name = PropertyBinding.parseTrackName(track.name).nodeName;
    const node = PropertyBinding.findNode(scene, name) as Object3D | null;
    if (!node || track.values.length < 3) return [];
    let depth = 0; for (let parent = node.parent; parent && parent !== scene; parent = parent.parent) depth++;
    return [{ node, track, depth }];
  }).sort((a, b) => a.depth - b.depth);
  const selected = candidates[0];
  if (!selected) return null;
  const interpolant = selected.track.createInterpolant(new Float32Array(3));
  const start = new Vector3().fromArray(interpolant.evaluate(selected.track.times[0]));
  const end = new Vector3().fromArray(interpolant.evaluate(selected.track.times[selected.track.times.length - 1]));
  const displacement = end.clone().sub(start);
  if (selected.node.parent) displacement.applyMatrix3(new Matrix3().setFromMatrix4(selected.node.parent.matrixWorld));
  const anchor = start.clone();
  if (selected.node.parent) anchor.applyMatrix4(selected.node.parent.matrixWorld);
  const position = new Vector3();
  const apply = (time: number, inPlace: boolean) => {
    position.fromArray(interpolant.evaluate(time));
    const parent = selected.node.parent;
    if (inPlace) {
      parent?.updateWorldMatrix(true,false);
      if (parent) position.applyMatrix4(parent.matrixWorld);
      position.x = anchor.x; position.z = anchor.z;
      if (parent) parent.worldToLocal(position);
    }
    // Resample explicitly: a paused mixer's property cache otherwise leaves
    // the clamped root in place when switching back to authored travel.
    selected.node.position.copy(position);
  };
  return { node: selected.node, start, anchor, displacement, apply };
}
