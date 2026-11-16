/**
 * Demo shim for services/reveal. "Reveal in Explorer" has no meaning in the
 * browser, so point visitors to the desktop app.
 */

import type { Asset } from "@app/domain/catalog";
import { desktopAction } from "../nativeNotice";

export async function revealAsset(_asset: Asset): Promise<void> { desktopAction(); }
