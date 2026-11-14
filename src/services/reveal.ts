/**
 * Reveal a library asset in the OS file manager. Resolves the asset's glTF (a
 * material's folder) to an absolute path (via Rust) and asks the opener plugin
 * to highlight it.
 */

import { revealItemInDir } from "@tauri-apps/plugin-opener";

import type { Asset } from "../domain/catalog";
import { resolveAssetPath } from "./tauriApi";

export async function revealAsset(asset: Asset): Promise<void> {
  const rel = asset.kind === "model" ? asset.fileset.gltf : asset.relPath;
  const abs = await resolveAssetPath(rel);
  await revealItemInDir(abs);
}
