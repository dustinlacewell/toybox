import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { humanoidRig } from "../packages/toybox-sdk/dist/index.js";

function moduleUrl(file, replacements = {}) {
  let { outputText } = ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  for (const [from, to] of Object.entries(replacements)) outputText = outputText.replaceAll(`from "${from}"`, `from ${JSON.stringify(to)}`);
  return `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
}
const sdk = { "@ldlework/toybox-sdk": import.meta.resolve("@ldlework/toybox-sdk") };
const manifestOf = (id) => JSON.parse(readFileSync(new URL(`../plugins/${id}/manifest.json`, import.meta.url), "utf8"));
// What the single com.toybox.humanoid-rigs plugin produced before it was split, captured from its index.ts.
const before = JSON.parse(readFileSync(new URL("./fixtures/humanoid-rigs-profiles.json", import.meta.url), "utf8"));
const rigs = {
  "com.toybox.rig-synty-polygon": ["synty-polygon", "Synty Polygon rig"],
  "com.toybox.rig-synty-sidekick": ["synty-sidekick", "Synty Sidekick rig"],
  "com.toybox.rig-synty-unreal": ["synty-unreal", "Synty Unreal rig"],
  "com.toybox.rig-quaternius": ["quaternius-humanoid", "Quaternius Humanoid rig"],
};
// Compared as JSON: that is what the host registers, and the old output carried `referencePoseNames: undefined` for two rigs.
const json = (value) => JSON.parse(JSON.stringify(value));

test("each rig plugin builds exactly the profile the humanoid-rigs plugin built for it", async () => {
  assert.deepEqual(before.map((profile) => profile.id), ["synty-polygon", "synty-sidekick", "quaternius-humanoid", "synty-unreal"]);
  for (const [pluginId, [profileId]] of Object.entries(rigs)) {
    const { default: plugin } = await import(moduleUrl(`../plugins/${pluginId}/index.ts`, sdk));
    assert.equal(plugin.profiles.length, 1, `${pluginId} ships one profile`);
    assert.deepEqual(json(plugin.profiles[0]), before.find((profile) => profile.id === profileId), pluginId);
  }
});

test("the four rig plugins together carry all four profiles, each under its own plugin", () => {
  assert.deepEqual(Object.values(rigs).map(([profileId]) => profileId).sort(), before.map((profile) => profile.id).sort());
  for (const [pluginId, [, name]] of Object.entries(rigs)) {
    const manifest = manifestOf(pluginId);
    assert.equal(manifest.name, name);
    assert.deepEqual(manifest.dependencies, { "com.toybox.model-core": "^3.0.0" });
    assert.equal(manifest.entry, "dist/index.js");
    assert.equal(manifest.assetTypes, undefined, "a rig plugin declares no types");
    const source = readFileSync(new URL(`../plugins/${pluginId}/index.ts`, import.meta.url), "utf8");
    assert.deepEqual([...source.matchAll(/from "([^"]+)"/g)].map((match) => match[1]), ["@ldlework/toybox-sdk"], "plugins import nothing but the SDK");
  }
});

test("humanoidRig builds the shared skeleton from bone names alone", () => {
  const side = (suffix) => ({
    clavicle: "c" + suffix, upperArm: ["ua" + suffix, "ua2" + suffix], forearm: "fa" + suffix, hand: "h" + suffix, upperLeg: "ul" + suffix, lowerLeg: "ll" + suffix, foot: "f" + suffix, toe: "t" + suffix,
    fingers: Object.fromEntries(["thumb", "index", "middle", "ring", "pinky"].map((finger) => [finger, [1, 2, 3].map((joint) => `${finger}${joint}${suffix}`)])),
  });
  const profile = humanoidRig({ id: "x", name: "X", torso: { hips: "H", spine: "S", chest: "C", neck: "N", head: "D" }, left: side("L"), right: side("R") });
  assert.equal(Object.keys(profile).includes("referencePoseNames"), false);
  assert.deepEqual(profile.bones.upperArmLeft, ["ua" + "L", "ua2L"], "a name or a list of aliases");
  assert.deepEqual(profile.bones.pinky3Right, ["pinky3R"]);
  assert.equal(Object.keys(profile.bones).length, 5 + 2 * (8 + 15));
  assert.deepEqual(profile.required.slice(0, 5), ["hips", "spine", "chest", "neck", "head"]);
  assert.equal(profile.required.length, 5 + 12);
  assert.deepEqual([profile.parents.spine, profile.parents.chest, profile.parents.neck, profile.parents.head], ["hips", "spine", "chest", "neck"]);
  assert.deepEqual([profile.parents.forearmLeft, profile.parents.handLeft, profile.parents.upperArmLeft, profile.parents.clavicleRight, profile.parents.toeRight], ["upperArmLeft", "forearmLeft", "chest", "chest", "footRight"]);
  assert.deepEqual([profile.parents.index1Left, profile.parents.index2Left, profile.parents.index3Right], ["handLeft", "index1Left", "index2Right"]);
  // Every parent is a mapped role, and profiles do not share state.
  assert.ok(Object.entries(profile.parents).every(([child, parent]) => profile.bones[child] && profile.bones[parent]));
  const again = humanoidRig({ id: "y", name: "Y", torso: { hips: "H", spine: "S", chest: "C", neck: "N", head: "D" }, left: side("L"), right: side("R"), referencePoseNames: ["Pose"] });
  again.parents.spine = "changed";
  assert.equal(profile.parents.spine, "hips");
  assert.deepEqual(again.referencePoseNames, ["Pose"]);
});
