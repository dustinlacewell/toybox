/**
 * Per-kind dispatch for the asset detail viewer. Each asset kind has its own
 * viewer; the lookup is keyed by AssetKind so the compiler rejects a kind
 * without one. Every viewer renders inside the shared ViewerFrame.
 */

import type { ComponentType } from "react";

import type { Asset, AssetKind } from "../domain/catalog";
import { ModelViewer } from "./ModelViewer";
import { ViewerFrame } from "./ViewerFrame";

interface DetailProps {
  asset: Asset;
  onClose: () => void;
}

type ViewerProps<K extends AssetKind> = {
  asset: Extract<Asset, { kind: K }>;
  onClose: () => void;
};

type ViewerLookup = { [K in AssetKind]: ComponentType<ViewerProps<K>> };

/** Placeholder until the material viewer exists: the frame with an empty stage. */
function MaterialViewer({ asset, onClose }: ViewerProps<"material">) {
  return <ViewerFrame asset={asset} onClose={onClose} />;
}

const VIEWERS: ViewerLookup = {
  model: ModelViewer,
  material: MaterialViewer,
};

export function AssetDetail({ asset, onClose }: DetailProps) {
  // The lookup guarantees VIEWERS[asset.kind] accepts the matching asset kind;
  // TypeScript cannot correlate the two through the union, so widen once here.
  const Viewer = VIEWERS[asset.kind] as ComponentType<DetailProps>;
  return <Viewer asset={asset} onClose={onClose} />;
}
