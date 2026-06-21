/**
 * Load the plugin registry once on mount and expose it to the views that render
 * plugin-contributed UI (the export drawer's mode list, a future import panel).
 * Discovery + per-plugin load isolation live in `services/pluginRegistry`.
 */

import { useEffect, useState } from "react";

import { loadRegistry, type PluginRegistry } from "../services/pluginRegistry";

const EMPTY: PluginRegistry = { exporters: [], importers: [], errors: [] };

export function usePluginRegistry(): PluginRegistry {
  const [registry, setRegistry] = useState<PluginRegistry>(EMPTY);

  useEffect(() => {
    let live = true;
    loadRegistry()
      .then((r) => {
        if (live) setRegistry(r);
      })
      .catch((e) => {
        // Discovery itself failing (not an individual plugin) leaves the empty
        // registry; native export modes still work.
        console.error("[plugins] discovery failed:", e);
      });
    return () => {
      live = false;
    };
  }, []);

  return registry;
}
