/**
 * Open a native save-file picker. Returns the chosen absolute path, or null if
 * the user cancelled. Used to choose the asset_placer `asset_library.json` to
 * create or merge. Uses the Tauri dialog plugin (services-layer only).
 */

import { save } from "@tauri-apps/plugin-dialog";

export async function pickSaveFile(defaultName: string): Promise<string | null> {
  const result = await save({
    defaultPath: defaultName,
    filters: [{ name: "asset_library", extensions: ["json"] }],
  });
  return typeof result === "string" ? result : null;
}
