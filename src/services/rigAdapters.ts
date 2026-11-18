import type { RigAdapterPlugin, RigProfile } from "@ldlework/toybox-sdk";
import polygon from "../../plugins/com.toybox.rig-synty-polygon/index";
import sidekick from "../../plugins/com.toybox.rig-synty-sidekick/index";
import quaternius from "../../plugins/com.toybox.rig-quaternius/index";
import unreal from "../../plugins/com.toybox.rig-synty-unreal/index";
import { mapRig } from "../domain/rigMapping";
import type { SceneAsset, AnimationAsset, Catalog } from "../domain/catalog";
/** The profiles the host carries built in: those of the bundled rig plugins, by
 *  plugin id, in the order they are tried. */
const BUILTIN: [string, RigAdapterPlugin][] = [
    ["com.toybox.rig-synty-polygon", polygon],
    ["com.toybox.rig-synty-sidekick", sidekick],
    ["com.toybox.rig-quaternius", quaternius],
    ["com.toybox.rig-synty-unreal", unreal],
];
export const BUILTIN_RIG_PLUGINS: string[] = BUILTIN.map(([id]) => id);
const builtinProfiles = (off: Iterable<string> = []) => { const skip = new Set(off); return BUILTIN.filter(([id]) => !skip.has(id)).flatMap(([, plugin]) => plugin.profiles); };
let profiles: RigProfile[] = builtinProfiles();
let adapterVersion=0;
const mappingCache=new WeakMap<object,{registry:Catalog["rigs"];profiles:RigProfile[];value:ReturnType<typeof mapRig>}>();
const referenceCache=new WeakMap<Catalog["assets"],Map<string,AnimationAsset|undefined>>();
const listeners = new Set<() => void>();
export const subscribeRigAdapters = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
export const getRigProfiles = () => profiles;
/** Forget every registered profile before plugins are loaded again. Each
 *  built-in profile is its rig plugin's, so it goes too when that plugin is
 *  turned off: `off` lists the ids of the rig plugins that are. */
export function resetRigAdapters(off: Iterable<string> = []) {
    profiles = builtinProfiles(off);
    adapterVersion++;
    listeners.forEach(listener => listener());
}
export function registerRigAdapters(plugin: RigAdapterPlugin) {
    if (!plugin || !Array.isArray(plugin.profiles) || !plugin.profiles.length)
        throw Error("Rig adapter must supply profiles");
    for (const p of plugin.profiles) {
        if (!p.id || !p.name || !p.bones || !Array.isArray(p.required) || !p.required.length || !p.parents || Object.values(p.bones).some(names => !Array.isArray(names) || !names.length || names.some(name => typeof name !== "string")))
            throw Error("Invalid rig profile");
        const required = ["hips", "spine", "chest", "neck", "head", ...["Left", "Right"].flatMap(side => ["upperArm", "forearm", "hand", "upperLeg", "lowerLeg", "foot"].map(role => role + side))];
        if (required.some(role => !p.required.includes(role) || !p.bones[role]))
            throw Error("Humanoid profiles must map torso, head, arms and legs");
        if (Object.entries(p.parents).some(([child, parent]) => !p.bones[child] || !p.bones[parent] || typeof parent !== "string"))
            throw Error("Invalid semantic parent relationship");
    }
    const incoming = new Set(plugin.profiles.map(profile => profile.id));
    if (incoming.size !== plugin.profiles.length)
        throw Error("Duplicate rig profile IDs");
    profiles = [...profiles.filter(profile => !incoming.has(profile.id)), ...structuredClone(plugin.profiles)];
    adapterVersion++;
    listeners.forEach(listener => listener());
}
export function assetMapping(asset: SceneAsset, catalog: Catalog) {
    if(!asset.rigs?.length)return null;
    const cached=mappingCache.get(asset.rigs);
    if(cached&&cached.registry===catalog.rigs&&cached.profiles===profiles)return cached.value;
    for (const binding of asset.rigs ?? []) {
        const rig = catalog.rigs?.find(rig => rig.id === binding.rigId);
        if (rig?.unambiguous && rig.kind === "skeleton") {
            const result = mapRig(rig.nodes, profiles);
            if (result) {
                mappingCache.set(asset.rigs,{registry:catalog.rigs,profiles,value:result});
                return result;
            }
        }
    }
    mappingCache.set(asset.rigs,{registry:catalog.rigs,profiles,value:null});return null;
}
export function referencePose(asset: SceneAsset, catalog: Catalog): AnimationAsset | undefined {
    const mapping = assetMapping(asset, catalog);
    if(!mapping)return undefined;
    let cache=referenceCache.get(catalog.assets);
    if(!cache){cache=new Map();referenceCache.set(catalog.assets,cache);}
    const key=`${adapterVersion}:${mapping.profile.id}`;
    if(cache.has(key))return cache.get(key);
    const result=catalog.assets.find((candidate): candidate is AnimationAsset => candidate.kind === "animation" && !!mapping.profile.referencePoseNames?.includes(candidate.name) && assetMapping(candidate, catalog)?.profile.id === mapping.profile.id);
    cache.set(key,result);return result;
}
