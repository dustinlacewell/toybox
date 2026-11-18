/**
 * The render-time host surface handed to a plugin's slot UI component (`ctx.host`).
 * Deliberately the narrow read+picker subset of the run-time `HostApi`/`FsApi`:
 * a panel reads the selection and opens OS pickers while rendering, but the
 * gated Rust write/export primitives fire later at run/commit (host-driven), so
 * they are not exposed during render.
 */

import type { PluginPermissions, SlotHost } from "@ldlework/toybox-sdk";
import { useStore } from "../state/store";
import { gate, catalogAsset, fetchAsset, loadSelectedAssets, selectedAssets } from "./pluginHost";
import { convertToGltf, listLibraries } from "./tauriApi";
import { pickDirectory } from "./pickDirectory";
import { pickSaveFile } from "./pickSaveFile";

/** The library a panel works in: its id, a getter for a choice that can change
 *  while the panel is open (the import drawer's "Into library"), or nothing for
 *  the default library. */
export type LibraryChoice = string | (() => string | undefined);

/** The library id a panel resolves to now; `null` when none is registered. */
async function resolveLibrary(choice: LibraryChoice | undefined): Promise<{ id: string; root: string } | null> {
  const wanted = typeof choice === "function" ? choice() : choice;
  const libraries = await listLibraries();
  const found = libraries.find((library) => library.id === wanted) ?? libraries.find((library) => library.isDefault) ?? libraries[0];
  return found ? { id: found.id, root: found.root } : null;
}

/** The render-time host for a slot panel. `perms`/`pluginId` gate the converter
 *  primitive (importers only); export panels pass nothing and never see it. */
export function buildSlotHost(
  pluginId = "",
  perms: PluginPermissions = {},
  library?: LibraryChoice,
): SlotHost {
  const gatedConvert = gate(pluginId, "rustConvert", perms.rustConvert);
  return {
    getSelectedAssets: () => selectedAssets([...useStore.getState().selection]),
    getAsset: (id) => catalogAsset(id),
    getSelectedAssetsAsync: () => loadSelectedAssets([...useStore.getState().selection]),
    getAssetAsync: (id) => fetchAsset(id),
    getLibraryRoot: async () => (await resolveLibrary(library))?.root ?? null,
    convertToGltf: gatedConvert(
      async (srcPath: string, pack: string, category: string, stem: string) => {
        const target = await resolveLibrary(library);
        if (!target) throw new Error("no library configured");
        return convertToGltf(target.id, srcPath, pack, category, stem);
      },
    ),
    pickDirectory,
    pickSaveFile,
  };
}
