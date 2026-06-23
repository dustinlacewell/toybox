/**
 * Open a native open-file picker. Returns the chosen absolute path, or null if
 * the user cancelled. Used to point at the FBX2glTF executable. Uses the Tauri
 * dialog plugin (services-layer only).
 */

import { open } from "@tauri-apps/plugin-dialog";

export async function pickFile(): Promise<string | null> {
  const result = await open({ directory: false, multiple: false });
  return typeof result === "string" ? result : null;
}
