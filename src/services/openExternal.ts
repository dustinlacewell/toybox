/** Open a web address in the system browser (through the opener plugin). */

import { openUrl } from "@tauri-apps/plugin-opener";

export async function openExternalUrl(url: string): Promise<void> {
  await openUrl(url);
}
