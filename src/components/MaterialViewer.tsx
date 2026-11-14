/**
 * Detail viewer for a single material asset, shown inside ViewerFrame. The stage
 * is a main area and a column of the material's maps. The main area shows an
 * interactive 3D preview of the material on a chosen shape, or, when a map is
 * selected, that texture file as a flat image. The material is loaded once per
 * asset and disposed by useMaterial when the viewer closes or switches asset.
 */

import { useState } from "react";

import type { MaterialAsset } from "../domain/catalog";
import { loadMaterial } from "../services/materialLoad";
import { ViewerFrame } from "./ViewerFrame";
import { MapImage } from "./material/MapImage";
import { MapList } from "./material/MapList";
import { MaterialStudio } from "./material/MaterialStudio";
import { useMaterial } from "./material/useMaterial";
import "./MaterialViewer.css";

interface Props {
  asset: MaterialAsset;
  onClose: () => void;
}

export function MaterialViewer({ asset, onClose }: Props) {
  return (
    <ViewerFrame asset={asset} onClose={onClose}>
      {/* Keyed so shape, tiling and map selection reset for each asset. */}
      <MaterialStage key={asset.id} asset={asset} />
    </ViewerFrame>
  );
}

function MaterialStage({ asset }: { asset: MaterialAsset }) {
  const state = useMaterial(asset.id, () => loadMaterial(asset));
  const [selected, setSelected] = useState<number | null>(null);

  const { maps } = asset.material;
  const map = selected === null ? null : maps[selected];

  return (
    <div className="matview">
      <div className="matview__main">
        {map ? <MapImage file={map.file} /> : <MaterialStudio state={state} />}
      </div>
      <MapList maps={maps} selected={selected} onSelect={setSelected} />
    </div>
  );
}
