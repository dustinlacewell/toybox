/**
 * Infinite ground grid at the world origin (Y=0), scaled to the model so cells
 * stay legible across the library's huge size range. Cells are a power-of-ten
 * step; every tenth line is a heavier "section" line. Fades with distance.
 */

import { Grid } from "@react-three/drei";
import { DoubleSide } from "three";

import { niceCellSize } from "./modelExtent";

interface Props {
  modelSize: number;
}

export function OriginGrid({ modelSize }: Props) {
  const cell = niceCellSize(modelSize);
  return (
    <Grid
      infiniteGrid
      cellSize={cell}
      sectionSize={cell * 10}
      cellThickness={0.45}
      sectionThickness={0.7}
      cellColor="#202020"
      sectionColor="#303030"
      fadeDistance={modelSize * 12}
      fadeStrength={1.5}
      followCamera={false}
      // Grid is a horizontal plane at the origin; render both sides so it's
      // visible whether the camera is above or below Y=0.
      side={DoubleSide}
    />
  );
}
