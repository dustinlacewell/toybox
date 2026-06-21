/**
 * The render-time host surface handed to a plugin's slot UI component (`ctx.host`).
 * Deliberately the narrow read+picker subset of the run-time `HostApi`/`FsApi`:
 * a panel reads the selection and opens OS pickers while rendering, but the
 * gated Rust write/export primitives fire later at run/commit (host-driven), so
 * they are not exposed during render.
 */

import type { SlotHost } from "@ldlework/toybox-sdk";
import { useStore } from "../state/store";
import { selectedAssets } from "./pluginHost";
import { pickDirectory } from "./pickDirectory";
import { pickSaveFile } from "./pickSaveFile";

export function buildSlotHost(): SlotHost {
  return {
    getSelectedAssets: () => selectedAssets([...useStore.getState().selection]),
    getAsset: (id) => useStore.getState().catalog?.assets.find((a) => a.id === id),
    pickDirectory,
    pickSaveFile,
  };
}
