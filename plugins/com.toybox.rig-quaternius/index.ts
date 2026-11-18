/**
 * Rig profile for the Quaternius humanoid (Universal Base Characters). The
 * humanoid skeleton is the SDK's; this file is only what Quaternius calls its
 * bones. It has no neutral-pose clip to recognise.
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
      id: "quaternius-humanoid",
      name: "Quaternius Humanoid",
      torso: { hips: "pelvis", spine: "spine_01", chest: "spine_03", neck: "neck_01", head: "Head" },
      left: side("l"),
      right: side("r"),
    }),
  ],
});
