/**
 * The services a plugin that drives an external program gets (`ctx.services`):
 * its own settings, the tools those settings point at, JSON requests to
 * services on this machine, and a scratch folder. The plugin id is bound here,
 * and the host checks that plugin's manifest for each permission natively; the
 * gates below only make a missing permission fail early with a clear message.
 */

import type { PluginPermissions, PluginServices } from "@ldlework/toybox-sdk";

import { gate } from "./pluginHost";
import {
  getPluginSettings, pluginHostProcessId, pluginLaunchTool, pluginLocalJson, pluginRunTool, pluginLocalPortOpen, pluginScratchDir, setPluginSetting,
} from "./tauriApi";

export function buildPluginServices(pluginId: string, perms: PluginPermissions = {}): PluginServices {
  const tools = gate(pluginId, "runTools", perms.runTools);
  const local = gate(pluginId, "localNetwork", perms.localNetwork);
  return {
    settings: {
      get: () => getPluginSettings(pluginId),
      set: (key, value) => setPluginSetting(pluginId, key, value),
    },
    tools: {
      launch: tools((setting: string, args: string[]) => pluginLaunchTool(pluginId, setting, args)),
      run: tools((setting: string, args: string[], options?: { timeoutMs?: number }) => pluginRunTool(pluginId, setting, args, options?.timeoutMs ?? 120_000)),
      hostProcessId: tools(() => pluginHostProcessId()),
    },
    local: {
      isOpen: local((port: number) => pluginLocalPortOpen(pluginId, port)),
      requestJson: local((port: number, message: unknown, timeoutMs = 30_000) => pluginLocalJson(pluginId, port, message, timeoutMs)),
    },
    scratchDir: (clear = false) => pluginScratchDir(pluginId, clear),
  };
}
