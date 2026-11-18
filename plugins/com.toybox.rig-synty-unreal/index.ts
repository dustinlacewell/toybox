/**
 * Rig profile for Synty characters on the Unreal mannequin skeleton. The
 * humanoid skeleton is the SDK's; this file is only what that skeleton calls
 * its bones: lower-case names, except the pelvis and the bones the mannequin
 * capitalises.
 */

import { defineRigAdapter, humanoidRig, type HumanoidFinger, type HumanoidSide } from "@ldlework/toybox-sdk";

const FINGERS: HumanoidFinger[] = ["thumb", "index", "middle", "ring", "pinky"];
const side = (lower: "l" | "r", upper: "L" | "R"): HumanoidSide => ({
  clavicle: `clavicle_${lower}`,
  upperArm: `UpperArm_${upper}`,
  forearm: `lowerarm_${lower}`,
  hand: `Hand_${upper}`,
  upperLeg: `Thigh_${upper}`,
  lowerLeg: `calf_${lower}`,
  foot: `Foot_${upper}`,
  toe: `ball_${lower}`,
  fingers: Object.fromEntries(FINGERS.map((name) => [name, [1, 2, 3].map((joint) => `${name}_0${joint}_${lower}`)])) as HumanoidSide["fingers"],
});

export default defineRigAdapter({
  profiles: [
    humanoidRig({
      id: "synty-unreal",
      name: "Synty Unreal",
      torso: { hips: "Pelvis", spine: "spine_01", chest: "spine_03", neck: "neck_01", head: "head" },
      left: side("l", "L"),
      right: side("r", "R"),
    }),
  ],
});
