/**
 * Demo shim for services/reveal. "Reveal in Explorer" has no meaning in the
 * browser, so the button is inert here.
 */

import type { Asset } from "@app/domain/catalog";

export async function revealAsset(_asset: Asset): Promise<void> {}
