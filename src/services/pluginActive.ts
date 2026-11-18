/**
 * Whether a plugin takes part in the app. A plugin is active when the user has
 * it switched on and every plugin it needs is usable. Only active plugins are
 * loaded and contribute types, importers, exporters, rig profiles or extensions.
 * Pure: no host imports.
 */

interface ActiveSource {
  enabled?: boolean;
  blockedBy?: unknown[];
}

export const isActive = (plugin: ActiveSource): boolean => plugin.enabled !== false && !plugin.blockedBy?.length;

/** Switched on by the user but held back by a dependency that is not usable. */
export const isBlocked = (plugin: ActiveSource): boolean => plugin.enabled !== false && !!plugin.blockedBy?.length;
