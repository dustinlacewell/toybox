/**
 * Turns what an entry module default-exports into a `PluginModule`. The export
 * is the module itself or a function of the plugin's context that returns it,
 * possibly asynchronously. Anything else is an error for that plugin alone.
 * Pure: no host imports.
 */

import type { PluginCtx, PluginModule } from "@ldlework/toybox-sdk";

export async function resolvePluginModule(id: string, exported: unknown, ctx: PluginCtx): Promise<PluginModule> {
  const value = typeof exported === "function" ? await (exported as (ctx: PluginCtx) => unknown)(ctx) : exported;
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`plugin '${id}' must default-export a module object or a function that returns one`);
  }
  const module = value as PluginModule;
  if (module.run !== undefined && typeof module.run !== "function") throw new Error(`plugin '${id}': run must be a function`);
  if (module.profiles !== undefined && !Array.isArray(module.profiles)) throw new Error(`plugin '${id}': profiles must be an array`);
  if (module.types !== undefined && (!module.types || typeof module.types !== "object" || Array.isArray(module.types))) {
    throw new Error(`plugin '${id}': types must be an object keyed by type id`);
  }
  return module;
}
