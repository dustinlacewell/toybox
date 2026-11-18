/**
 * Load the plugin registry on mount and expose it to the views that render
 * plugin-contributed UI. `reload` discovers plugins again — after one is turned
 * on or off in settings. Discovery + per-plugin load isolation live in
 * `services/pluginRegistry`.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { emptyRegistry, loadRegistry, type PluginRegistry } from "../services/pluginRegistry";
import { markAssetTypesLoaded, useAssetTypes } from "../state/assetTypes";
import { isActive } from "../services/pluginActive";
import { usePlugins } from "../state/plugins";

const EMPTY: PluginRegistry = emptyRegistry();

export function usePluginRegistry(): PluginRegistry & { reload: () => Promise<void> } {
  const [registry, setRegistry] = useState<PluginRegistry>(EMPTY);
  // Only the most recent discovery may publish its result.
  const run = useRef(0);

  const reload = useCallback(async () => {
    const current = ++run.current;
    try {
      const loaded = await loadRegistry();
      if (current !== run.current) return;
      useAssetTypes.getState().setTypes(loaded.types);
      usePlugins.getState().setEnabled(loaded.plugins.filter(isActive));
      usePlugins.getState().setRules(loaded.rules);
      setRegistry(loaded);
    } catch (e) {
      // Discovery itself failing (not an individual plugin) leaves the previous
      // registry; native export modes still work.
      console.error("[plugins] discovery failed:", e);
    } finally {
      markAssetTypesLoaded();
    }
  }, []);

  useEffect(() => {
    void reload();
    return () => { run.current++; };
  }, [reload]);

  return { ...registry, reload };
}
