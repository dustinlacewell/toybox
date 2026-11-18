/**
 * The pieces of host every plugin context is made from: the render-time host,
 * the jailed fs, the services and the extension lookup, all bound to one
 * plugin and its permissions. The entry module, the import and export panels
 * and the asset panel each start from this.
 */

import type { ExtensionLookup, FsApi, PluginCtx, PluginPermissions, PluginServices, SlotHost } from "@ldlework/toybox-sdk";

import { extensionLookup } from "./pluginExtensions";
import { buildFsApi } from "./pluginHost";
import { buildPluginServices } from "./pluginServices";
import { buildSlotHost, type LibraryChoice } from "./slotHost";

export interface PluginBase {
  host: SlotHost;
  fs: FsApi;
  services: PluginServices;
  extensions: ExtensionLookup;
}

/** `library` is the library the panel works in (an id, or a getter for a choice
 *  that can change): `host.getLibraryRoot()` and the converter act on it. Absent,
 *  they act on the default library. */
export function buildPluginBase(pluginId = "", perms: PluginPermissions = {}, library?: LibraryChoice): PluginBase {
  return {
    host: buildSlotHost(pluginId, perms, library),
    fs: buildFsApi(pluginId, perms),
    services: buildPluginServices(pluginId, perms),
    extensions: extensionLookup,
  };
}

/** The context an entry module (or its factory function) receives. */
export function buildPluginCtx(pluginId: string, perms: PluginPermissions = {}): PluginCtx {
  return { id: pluginId, ...buildPluginBase(pluginId, perms) };
}
