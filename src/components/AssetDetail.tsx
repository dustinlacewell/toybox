/**
 * Per-kind dispatch for the asset detail viewer. Each asset kind has its own
 * viewer; the lookup is keyed by AssetKind so the compiler rejects a kind
 * without one. Every viewer renders inside the shared ViewerFrame.
 */

import type { ComponentType } from "react";

import type { Asset, AssetKind } from "../domain/catalog";
import { ModelViewer } from "./ModelViewer";
import { MaterialViewer } from "./MaterialViewer";
import { AnimationViewer } from "./AnimationViewer";
import { AudioViewer } from "./AudioViewer";

interface DetailProps {
  onExport?: () => void;
  asset: Asset;
  onClose: () => void;
}

type ViewerProps<K extends AssetKind> = {
  onExport?: () => void;
  asset: Extract<Asset, { kind: K }>;
  onClose: () => void;
};

type ViewerLookup = { [K in AssetKind]: ComponentType<ViewerProps<K>> };

const VIEWERS: ViewerLookup = {
  model: ModelViewer,
  material: MaterialViewer,
  animation: AnimationViewer,
  audio: AudioViewer,
};

export function AssetDetail({ asset, onClose, onExport }: DetailProps) {
  // The lookup guarantees VIEWERS[asset.kind] accepts the matching asset kind;
  // TypeScript cannot correlate the two through the union, so widen once here.
  const Viewer = VIEWERS[asset.kind] as ComponentType<DetailProps>;
  return <Viewer key={asset.id} asset={asset} onClose={onClose} onExport={onExport} />;
}
