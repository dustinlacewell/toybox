import { defineRigAdapter, type RigProfile } from "@ldlework/toybox-sdk";
const required = ["hips", "spine", "chest", "neck", "head", ...["Left", "Right"].flatMap(side => ["upperArm", "forearm", "hand", "upperLeg", "lowerLeg", "foot"].map(bone => bone + side))];
const parents: Record<string, string> = { spine: "hips", chest: "spine", neck: "chest", head: "neck" };
for (const side of ["Left", "Right"])
    Object.assign(parents, { ["clavicle" + side]: "chest", ["upperArm" + side]: "chest", ["forearm" + side]: "upperArm" + side, ["hand" + side]: "forearm" + side, ["upperLeg" + side]: "hips", ["lowerLeg" + side]: "upperLeg" + side, ["foot" + side]: "lowerLeg" + side, ["toe" + side]: "foot" + side });
function profile(id: string, name: string, polygon: boolean, head: string, referencePoseNames?: string[]): RigProfile {
    const bones: Record<string, string[]> = polygon ? { hips: ["Hips"], spine: ["Spine_01"], chest: ["Spine_03"], neck: ["Neck"], head: [head] } : { hips: ["pelvis"], spine: ["spine_01"], chest: ["spine_03"], neck: ["neck_01"], head: [head] };
    for (const [side, suffix] of [["Left", "l"], ["Right", "r"]]) {
        const names = polygon ? { clavicle: "Clavicle", upperArm: "Shoulder", forearm: "Elbow", hand: "Hand", upperLeg: "UpperLeg", lowerLeg: "LowerLeg", foot: "Ankle", toe: "Ball" } : { clavicle: "clavicle", upperArm: "upperarm", forearm: "lowerarm", hand: "hand", upperLeg: "thigh", lowerLeg: "calf", foot: "foot", toe: "ball" };
        for (const [role, bone] of Object.entries(names))
            bones[role + side] = [bone + "_" + (polygon ? suffix.toUpperCase() : suffix)];
        for (const [finger, poly, other] of [["thumb", "Thumb", "thumb"], ["index", "IndexFinger", "index"], ["middle", "Finger", "middle"], ["ring", "Finger", "ring"], ["pinky", "Finger", "pinky"]]) {
            for (let joint = 1; joint <= 3; joint++) {
                const role = `${finger}${joint}${side}`;
                bones[role] = polygon ? (side === "Left" ? [`${poly}_0${joint}`] : [`${poly}_0${joint}_1`, `${poly}_0${joint}_R`]) : [`${other}_0${joint}_${suffix}`];
                parents[role] = joint === 1 ? "hand" + side : `${finger}${joint - 1}${side}`;
            }
        }
    }
    return { id, name, bones, required, parents: { ...parents }, referencePoseNames };
}
const unreal = profile("synty-unreal", "Synty Unreal", false, "head");
unreal.bones.hips = ["Pelvis"];
for (const [side, suffix] of [["Left", "L"], ["Right", "R"]]) {
    for (const [role, name] of Object.entries({ upperArm: "UpperArm", hand: "Hand", upperLeg: "Thigh", foot: "Foot" }))
        unreal.bones[role + side] = [name + "_" + suffix];
}
export default defineRigAdapter({ profiles: [
        profile("synty-polygon", "Synty Polygon", true, "Head", ["A_TPose_Neut"]),
        profile("synty-sidekick", "Synty Sidekick", false, "head", ["A_MOD_BL_TPose_Neut"]),
        profile("quaternius-humanoid", "Quaternius Humanoid", false, "Head"),
        unreal,
    ] });
