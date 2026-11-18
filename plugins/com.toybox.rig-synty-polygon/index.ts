/**
 * Rig profile for Synty Polygon characters. The humanoid skeleton is the SDK's;
 * this file is only what Polygon calls its bones. The neutral pose is the
 * `A_TPose_Neut` clip of the user's own Synty animation pack.
 */

import { defineRigAdapter, humanoidRig, type HumanoidFinger, type HumanoidSide } from "@ldlework/toybox-sdk";

// The right hand names its finger joints twice over: some packs export the
// `_1` copy of the joint and some the `_R` one, so both are aliases.
const finger = (name: string, right: boolean): [string[], string[], string[]] =>
  [1, 2, 3].map((joint) => (right ? [`${name}_0${joint}_1`, `${name}_0${joint}_R`] : [`${name}_0${joint}`])) as [string[], string[], string[]];
const fingers = (right: boolean): HumanoidSide["fingers"] => {
  const names: Record<HumanoidFinger, string> = { thumb: "Thumb", index: "IndexFinger", middle: "Finger", ring: "Finger", pinky: "Finger" };
  return Object.fromEntries(Object.entries(names).map(([role, name]) => [role, finger(name, right)])) as HumanoidSide["fingers"];
};
const side = (suffix: "L" | "R"): HumanoidSide => ({
  clavicle: `Clavicle_${suffix}`,
  upperArm: `Shoulder_${suffix}`,
  forearm: `Elbow_${suffix}`,
  hand: `Hand_${suffix}`,
  upperLeg: `UpperLeg_${suffix}`,
  lowerLeg: `LowerLeg_${suffix}`,
  foot: `Ankle_${suffix}`,
  toe: `Ball_${suffix}`,
  fingers: fingers(suffix === "R"),
});

export default defineRigAdapter({
  profiles: [
    humanoidRig({
      id: "synty-polygon",
      name: "Synty Polygon",
      torso: { hips: "Hips", spine: "Spine_01", chest: "Spine_03", neck: "Neck", head: "Head" },
      left: side("L"),
      right: side("R"),
      referencePoseNames: ["A_TPose_Neut"],
    }),
  ],
});
