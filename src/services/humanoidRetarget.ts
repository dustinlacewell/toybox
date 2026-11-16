import { AnimationClip, AnimationMixer, LoopOnce, Quaternion, QuaternionKeyframeTrack, Vector3, VectorKeyframeTrack, SkinnedMesh, type Group, type Object3D, type KeyframeTrack } from "three";
import type { RigProfile } from "@ldlework/toybox-sdk";
import { mapRig, type HumanoidMapping, type MappingNode } from "../domain/rigMapping";
export function sceneMapping(scene: Object3D, profiles: RigProfile[]) {
    const objects = new Map<string, Object3D>();
    const nodes: MappingNode[] = [];
    function visit(node: Object3D, parent: string | null) {
        const key = (parent ? parent + "/" : "") + node.name.replace(/~/g, "~0").replace(/\//g, "~1");
        objects.set(key, node);
        nodes.push({ key, name: node.name, parent });
        node.children.forEach(child => visit(child, key));
    }
    visit(scene, null);
    if (objects.size !== nodes.length)
        throw Error("Duplicate node paths prevent safe rig mapping");
    const mapping = mapRig(nodes, profiles);
    if (!mapping)
        throw Error("No unambiguous humanoid rig adapter matches this hierarchy");
    return { mapping, objects };
}
function bone(mapping: HumanoidMapping, objects: Map<string, Object3D>, role: string): Object3D {
    const result = objects.get(mapping.bones[role]);
    if (!result)
        throw Error(`Missing mapped bone: ${role}`);
    return result;
}
const childRole: Record<string, string> = { hips: "spine", spine: "chest", chest: "neck", neck: "head" };
for (const side of ["Left", "Right"]) {
    Object.assign(childRole, { ["clavicle" + side]: "upperArm" + side, ["upperArm" + side]: "forearm" + side, ["forearm" + side]: "hand" + side, ["hand" + side]: "middle1" + side, ["upperLeg" + side]: "lowerLeg" + side, ["lowerLeg" + side]: "foot" + side, ["foot" + side]: "toe" + side });
    for (const finger of ["thumb", "index", "middle", "ring", "pinky"])
        for (let i = 1; i < 3; i++)
            childRole[`${finger}${i}${side}`] = `${finger}${i + 1}${side}`;
}
/** Bake a display-only clip. Neither input asset nor exported animation changes.
 * Named mappings are explicit; rest-axis offsets, proportions and hips motion
 * belong to this shared solver rather than individual adapter plugins. */
export async function retargetHumanoid(source: Group, clip: AnimationClip, reference: Group, target: Group, profiles: RigProfile[], sampleTime?:number,signal?:AbortSignal) {
    signal?.throwIfAborted();
    const posed = new Set();
    target.traverse(node => { if (node instanceof SkinnedMesh && !posed.has(node.skeleton)) {
        posed.add(node.skeleton);
        node.skeleton.pose();
    } });
    const s = sceneMapping(source, profiles), r = sceneMapping(reference, profiles), t = sceneMapping(target, profiles);
    if (s.mapping.profile.id !== r.mapping.profile.id)
        throw Error("The neutral pose belongs to a different rig adapter");
    source.updateWorldMatrix(true, true);
    reference.updateWorldMatrix(true, true);
    target.updateWorldMatrix(true, true);
    const position = (data: typeof s, role: string) => bone(data.mapping, data.objects, role).getWorldPosition(new Vector3());
    // Align the two humanoids' lateral and vertical frames, preserving facing.
    const up = (data: typeof s) => position(data, "head").sub(position(data, "hips")).normalize();
    const left = (data: typeof s) => position(data, "upperArmLeft").sub(position(data, "upperArmRight")).normalize();
    const frame = (data: typeof s) => {
        const y = up(data), x = left(data);
        x.addScaledVector(y, -x.dot(y)).normalize();
        const z = x.clone().cross(y).normalize();
        // Quaternion from an orthonormal frame without a world-space scale.
        const q = new Quaternion();
        const matrix = bone(data.mapping, data.objects, "hips").matrixWorld.clone().identity();
        matrix.makeBasis(x, y, z);
        return q.setFromRotationMatrix(matrix);
    };
    const correction = frame(t).multiply(frame(r).invert());
    const sourceHip = position(r, "hips"), targetHip = position(t, "hips");
    const legLength = (data: typeof s) => position(data, "upperLegLeft").distanceTo(position(data, "lowerLegLeft")) + position(data, "lowerLegLeft").distanceTo(position(data, "footLeft"));
    const ratio = legLength(t) / legLength(r);
    if (!Number.isFinite(ratio) || ratio < .05 || ratio > 20)
        throw Error("Humanoid proportions are outside the supported preview range");
    const saved: {
        node: Object3D;
        position: Vector3;
        rotation: Quaternion;
    }[] = [];
    const pairs = Object.keys(t.mapping.bones).flatMap(role => {
        if (!s.mapping.bones[role] || !r.mapping.bones[role])
            return [];
        const a = bone(s.mapping, s.objects, role), neutral = bone(r.mapping, r.objects, role), b = bone(t.mapping, t.objects, role);
        const sourceRotation = neutral.getWorldQuaternion(new Quaternion()).premultiply(correction);
        const targetRotation = b.getWorldQuaternion(new Quaternion());
        const alignment = new Quaternion();
        const next = childRole[role];
        if (next && r.mapping.bones[next] && t.mapping.bones[next]) {
            const from = position(t, next).sub(b.getWorldPosition(new Vector3()));
            const to = position(r, next).sub(neutral.getWorldPosition(new Vector3())).applyQuaternion(correction);
            if (from.lengthSq() > 1e-10 && to.lengthSq() > 1e-10)
                alignment.setFromUnitVectors(from.normalize(), to.normalize());
        }
        const offset = sourceRotation.invert().multiply(alignment).multiply(targetRotation);
        let depth = 0;
        for (let parent = b.parent; parent; parent = parent.parent)
            depth++;
        saved.push({ node: b, position: b.position.clone(), rotation: b.quaternion.clone() });
        return [{ role, source: a, target: b, offset, depth, rotations: [] as number[], positions: [] as number[] }];
    }).sort((a, b) => a.depth - b.depth);
    const mixer = new AnimationMixer(source);
    const action = mixer.clipAction(clip);
    action.setLoop(LoopOnce, 1);
    action.clampWhenFinished = true;
    action.play();
    action.paused = true;
    const times: number[] = [];
    const frames = sampleTime===undefined ? Math.max(2, Math.min(10000, Math.ceil(Math.max(clip.duration, 1 / 30) * 30) + 1)) : 1;
    const desired = new Quaternion(), parentRotation = new Quaternion(), point = new Vector3();
    try {
        for (let index = 0; index < frames; index++) {
            signal?.throwIfAborted();
            const time = sampleTime===undefined ? clip.duration * index / (frames - 1) : Math.max(0,Math.min(clip.duration,sampleTime));
            times.push(sampleTime===undefined?time:0);
            action.time = time;
            mixer.update(0);
            source.updateWorldMatrix(true, true);
            for (const pair of pairs) {
                desired.copy(pair.source.getWorldQuaternion(new Quaternion())).premultiply(correction).multiply(pair.offset);
                const parent = pair.target.parent;
                parent?.updateWorldMatrix(true, false);
                if (parent)
                    desired.premultiply(parent.getWorldQuaternion(parentRotation).invert());
                pair.target.quaternion.copy(desired).normalize();
                if (pair.role === "hips") {
                    pair.source.getWorldPosition(point).sub(sourceHip).applyQuaternion(correction).multiplyScalar(ratio).add(targetHip);
                    if (parent)
                        parent.worldToLocal(point);
                    pair.target.position.copy(point);
                    pair.target.position.toArray(pair.positions, pair.positions.length);
                }
                pair.target.quaternion.toArray(pair.rotations, pair.rotations.length);
                pair.target.updateWorldMatrix(false, false);
            }
            if (index > 0 && index % 60 === 0)
                await new Promise(resolve => setTimeout(resolve, 0));
        }
        const tracks: KeyframeTrack[] = pairs.flatMap(pair => {
            const result: KeyframeTrack[] = [new QuaternionKeyframeTrack(`${pair.target.uuid}.quaternion`, times, pair.rotations)];
            if (pair.positions.length)
                result.push(new VectorKeyframeTrack(`${pair.target.uuid}.position`, times, pair.positions));
            return result;
        });
        return { clip: new AnimationClip(clip.name, clip.duration, tracks), sourceProfile: s.mapping.profile.name, targetProfile: t.mapping.profile.name };
    }
    finally {
        mixer.stopAllAction();
        mixer.uncacheRoot(source);
        for (const pose of saved) {
            pose.node.position.copy(pose.position);
            pose.node.quaternion.copy(pose.rotation);
        }
        target.updateWorldMatrix(true, true);
    }
}
