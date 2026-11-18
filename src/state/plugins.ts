/**
 * The installed plugins that are active (switched on, nothing they need
 * missing), for components outside the library view that need a plugin's
 * manifest — the viewer looks up the panel of the plugin an asset came from.
 * Filled by plugin discovery.
 */

import { create } from "zustand";

import { NO_RULES, type VisibilityRules } from "../domain/browseQuery";
import type { PluginManifestDto } from "../services/tauriApi";

interface PluginsState {
  enabled: PluginManifestDto[];
  setEnabled(plugins: PluginManifestDto[]): void;
  /** Which library items are hidden because their plugin is off, as the lists a query takes. */
  rules: VisibilityRules;
  setRules(rules: VisibilityRules): void;
}

export const usePlugins = create<PluginsState>((set) => ({
  enabled: [],
  setEnabled: (enabled) => set({ enabled }),
  rules: NO_RULES,
  setRules: (rules) => set((state) => (JSON.stringify(state.rules) === JSON.stringify(rules) ? {} : { rules })),
}));
