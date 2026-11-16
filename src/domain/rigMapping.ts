import type { RigProfile } from "@ldlework/toybox-sdk";
export interface MappingNode {
    key: string;
    name: string;
    parent: string | null;
}
export interface HumanoidMapping {
    profile: RigProfile;
    bones: Record<string, string>;
}
/** Missing, duplicate or incorrectly parented required bones prevent detection. */
export function mapRig(nodes: MappingNode[], profiles: RigProfile[]): HumanoidMapping | null {
    const results: HumanoidMapping[] = [];
    for (const profile of profiles) {
        const bones: Record<string, string> = {};
        for (const [role, aliases] of Object.entries(profile.bones)) {
            const matches = nodes.filter(node => aliases.includes(node.name));
            if (matches.length === 1)
                bones[role] = matches[0].key;
        }
        if (profile.required.some(role => !bones[role]))
            continue;
        const byKey = new Map(nodes.map(node => [node.key, node]));
        const valid = Object.entries(profile.parents).every(([child, parent]) => {
            if (!bones[child] || !bones[parent])
                return true;
            let current = byKey.get(bones[child])?.parent;
            const seen = new Set<string>();
            while (current && !seen.has(current)) {
                if (current === bones[parent])
                    return true;
                seen.add(current);
                current = byKey.get(current)?.parent;
            }
            return false;
        });
        if (valid)
            results.push({ profile, bones });
    }
    return results.length === 1 ? results[0] : null;
}
