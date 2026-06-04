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
      cellThickness={0.6}
      sectionThickness={1}
      cellColor="#3a4150"
      sectionColor="#5b6577"
      fadeDistance={modelSize * 30}
      fadeStrength={1.5}
      followCamera={false}
      // Grid is a horizontal plane at the origin; render both sides so it's
      // visible whether the camera is above or below Y=0.
      side={DoubleSide}
    />
  );
}
