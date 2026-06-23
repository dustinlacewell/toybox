/**
 * The render-time host surface handed to a plugin's slot UI component (`ctx.host`).
 * Deliberately the narrow read+picker subset of the run-time `HostApi`/`FsApi`:
 * a panel reads the selection and opens OS pickers while rendering, but the
 * gated Rust write/export primitives fire later at run/commit (host-driven), so
 * they are not exposed during render.
 */

import type { PluginPermissions, SlotHost } from "@ldlework/toybox-sdk";
import { useStore } from "../state/store";
import { gate, selectedAssets } from "./pluginHost";
import { convertToGltf, getLibraryRoot } from "./tauriApi";
import { pickDirectory } from "./pickDirectory";
import { pickSaveFile } from "./pickSaveFile";

/** The render-time host for a slot panel. `perms`/`pluginId` gate the converter
 *  primitive (importers only); export panels pass nothing and never see it. */
export function buildSlotHost(
  pluginId = "",
  perms: PluginPermissions = {},
): SlotHost {
  const gatedConvert = gate(pluginId, "rustConvert", perms.rustConvert);
  return {
    getSelectedAssets: () => selectedAssets([...useStore.getState().selection]),
    getAsset: (id) => useStore.getState().catalog?.assets.find((a) => a.id === id),
    getLibraryRoot: () => getLibraryRoot(),
    convertToGltf: gatedConvert(
      (srcPath: string, pack: string, category: string, stem: string) =>
        convertToGltf(srcPath, pack, category, stem),
    ),
    pickDirectory,
    pickSaveFile,
  };
}
