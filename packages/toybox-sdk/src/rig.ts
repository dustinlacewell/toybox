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
