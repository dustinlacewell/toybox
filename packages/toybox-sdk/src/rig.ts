/** A rig adapter supplies data. The host validates mappings and owns playback. */
export interface RigProfile {
    id: string;
    name: string;
    /** Exact, case-sensitive aliases. Multiple matches are ambiguous. */
    bones: Record<string, string[]>;
    required: string[];
    /** Expected nearest mapped ancestor, allowing unlisted helper bones. */
    parents: Record<string, string>;
    /** Optional neutral-pose animation in the user's library; never downloaded. */
    referencePoseNames?: string[];
}
export interface RigAdapterPlugin {
    profiles: RigProfile[];
}
export function defineRigAdapter(plugin: RigAdapterPlugin): RigAdapterPlugin { return plugin; }

/** One name, or several aliases when a rig names a bone more than one way. */
export type BoneNames = string | string[];
export type HumanoidFinger = "thumb" | "index" | "middle" | "ring" | "pinky";
/** The bones of one side of a humanoid. `fingers` lists the three joints of each finger, root first. */
export interface HumanoidSide {
    clavicle: BoneNames;
    upperArm: BoneNames;
    forearm: BoneNames;
    hand: BoneNames;
    upperLeg: BoneNames;
    lowerLeg: BoneNames;
    foot: BoneNames;
    toe: BoneNames;
    fingers: Record<HumanoidFinger, [BoneNames, BoneNames, BoneNames]>;
}
/** The bone names of one humanoid rig. Everything else about the skeleton is the same for every humanoid. */
export interface HumanoidRigSpec {
    id: string;
    name: string;
    torso: { hips: BoneNames; spine: BoneNames; chest: BoneNames; neck: BoneNames; head: BoneNames };
    left: HumanoidSide;
    right: HumanoidSide;
    referencePoseNames?: string[];
}
const FINGERS: readonly HumanoidFinger[] = ["thumb", "index", "middle", "ring", "pinky"];
const LIMBS = ["upperArm", "forearm", "hand", "upperLeg", "lowerLeg", "foot"] as const;
const aliases = (names: BoneNames): string[] => (Array.isArray(names) ? [...names] : [names]);

/**
 * Build a humanoid `RigProfile` from the bone names of one rig. The humanoid
 * skeleton itself is shared by every rig and lives here, so a rig plugin is just
 * its names: the required roles (hips, spine, chest, neck, head and, per side,
 * arm and leg down to the hand and foot), the semantic parent of every role
 * (spine under hips, forearm under upper arm, finger joints under the one
 * before, and so on) and the role names themselves (`upperArmLeft`,
 * `index2Right`, ...). The host validates the result when it registers it.
 */
export function humanoidRig(spec: HumanoidRigSpec): RigProfile {
    const bones: Record<string, string[]> = {};
    const parents: Record<string, string> = { spine: "hips", chest: "spine", neck: "chest", head: "neck" };
    for (const [role, names] of Object.entries(spec.torso)) bones[role] = aliases(names);
    for (const [side, names] of [["Left", spec.left], ["Right", spec.right]] as const) {
        for (const role of ["clavicle", ...LIMBS, "toe"] as const) bones[role + side] = aliases(names[role]);
        Object.assign(parents, {
            ["clavicle" + side]: "chest", ["upperArm" + side]: "chest", ["forearm" + side]: "upperArm" + side, ["hand" + side]: "forearm" + side,
            ["upperLeg" + side]: "hips", ["lowerLeg" + side]: "upperLeg" + side, ["foot" + side]: "lowerLeg" + side, ["toe" + side]: "foot" + side,
        });
        for (const finger of FINGERS) names.fingers[finger].forEach((joint, index) => {
            const role = `${finger}${index + 1}${side}`;
            bones[role] = aliases(joint);
            parents[role] = index === 0 ? "hand" + side : `${finger}${index}${side}`;
        });
    }
    const required = ["hips", "spine", "chest", "neck", "head", ...["Left", "Right"].flatMap(side => LIMBS.map(role => role + side))];
    const profile: RigProfile = { id: spec.id, name: spec.name, bones, required, parents };
    if (spec.referencePoseNames) profile.referencePoseNames = [...spec.referencePoseNames];
    return profile;
}
