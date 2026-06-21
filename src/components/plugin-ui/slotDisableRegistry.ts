/**
 * Per-session throw counter for plugin slot components. Third-party React in the
 * host tree will crash; after `LIMIT` throws in a session a given (plugin, slot)
 * stops being re-mounted so a persistently-broken panel can't thrash. Cleared
 * only by restarting the app (a session-scoped, in-memory map).
 */

const LIMIT = 3;
const throwsByKey = new Map<string, number>();

const key = (pluginId: string, moduleRel: string) => `${pluginId}::${moduleRel}`;

/** Record a throw; returns true once the slot has reached the disable limit. */
export function recordSlotThrow(pluginId: string, moduleRel: string): boolean {
  const k = key(pluginId, moduleRel);
  const next = (throwsByKey.get(k) ?? 0) + 1;
  throwsByKey.set(k, next);
  return next >= LIMIT;
}

/** Whether this slot has thrown enough to be disabled for the session. */
export function isSlotDisabled(pluginId: string, moduleRel: string): boolean {
  return (throwsByKey.get(key(pluginId, moduleRel)) ?? 0) >= LIMIT;
}
