/**
 * Per-kind dispatch for the asset detail viewer. Each native kind has its own
 * viewer; the lookup is keyed by the asset's kind tag so the compiler rejects a
 * kind without one. A plugin-defined asset goes to the plugin viewer host,
 * which mounts the type's own viewer. Every viewer renders inside the shared
 * ViewerFrame.
 */

import type { ComponentType } from "react";

import type { Asset } from "../domain/catalog";
import { ModelViewer } from "./ModelViewer";
import { MaterialViewer } from "./MaterialViewer";
import { AnimationViewer } from "./AnimationViewer";
import { AudioViewer } from "./AudioViewer";
import { PluginAssetViewer } from "./PluginAssetViewer";

interface DetailProps {
  onExport?: () => void;
  asset: Asset;
  onClose: () => void;
}

type ViewerProps<K extends Asset["kind"]> = {
  onExport?: () => void;
  asset: Extract<Asset, { kind: K }>;
  onClose: () => void;
};

type ViewerLookup = { [K in Asset["kind"]]: ComponentType<ViewerProps<K>> };

const VIEWERS: ViewerLookup = {
  model: ModelViewer,
  material: MaterialViewer,
  animation: AnimationViewer,
  audio: AudioViewer,
  plugin: PluginAssetViewer,
};

export function AssetDetail({ asset, onClose, onExport }: DetailProps) {
  // The lookup guarantees VIEWERS[asset.kind] accepts the matching asset kind;
  // TypeScript cannot correlate the two through the union, so widen once here.
  const Viewer = VIEWERS[asset.kind] as ComponentType<DetailProps>;
  return <Viewer key={asset.id} asset={asset} onClose={onClose} onExport={onExport} />;
}
