/**
 * Open a native folder picker. Returns the chosen absolute path, or null if the
 * user cancelled. Uses the Tauri dialog plugin (services-layer only).
 */

import { open } from "@tauri-apps/plugin-dialog";

export async function pickDirectory(): Promise<string | null> {
  const result = await open({ directory: true, multiple: false });
  return typeof result === "string" ? result : null;
}
