/**
 * Rig profile for Synty Sidekick characters. The humanoid skeleton is the SDK's;
 * this file is only what Sidekick calls its bones. The neutral pose is the
 * `A_MOD_BL_TPose_Neut` clip of the user's own Synty animation pack.
 */

import { defineRigAdapter, humanoidRig, type HumanoidFinger, type HumanoidSide } from "@ldlework/toybox-sdk";

const FINGERS: HumanoidFinger[] = ["thumb", "index", "middle", "ring", "pinky"];
const side = (suffix: "l" | "r"): HumanoidSide => ({
  clavicle: `clavicle_${suffix}`,
  upperArm: `upperarm_${suffix}`,
  forearm: `lowerarm_${suffix}`,
  hand: `hand_${suffix}`,
  upperLeg: `thigh_${suffix}`,
  lowerLeg: `calf_${suffix}`,
  foot: `foot_${suffix}`,
  toe: `ball_${suffix}`,
  fingers: Object.fromEntries(FINGERS.map((name) => [name, [1, 2, 3].map((joint) => `${name}_0${joint}_${suffix}`)])) as HumanoidSide["fingers"],
});

export default defineRigAdapter({
  profiles: [
    humanoidRig({
      id: "synty-sidekick",
      name: "Synty Sidekick",
      torso: { hips: "pelvis", spine: "spine_01", chest: "spine_03", neck: "neck_01", head: "head" },
      left: side("l"),
      right: side("r"),
      referencePoseNames: ["A_MOD_BL_TPose_Neut"],
    }),
  ],
});
