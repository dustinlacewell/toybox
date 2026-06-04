/**
 * Marks the geometry's bounding-box center — where the mesh actually sits. This
 * is the primary "spot the problem" indicator: when it drifts far from the
 * origin/pivot, the asset is mispositioned. Drawn as a small yellow 3-axis
 * crosshair, always on top so it's visible inside dense geometry.
 */

import { useMemo } from "react";
import { BufferGeometry, Float32BufferAttribute } from "three";

interface Props {
  position: [number, number, number];
  /** Reference size (model radius); the crosshair is a small fraction of it. */
  scale: number;
}

const COLOR = "#f8e71c"; // yellow — reserved for geometry center

export function GeometryCenterMarker({ position, scale }: Props) {
  const r = scale * 0.06;
  const geometry = useMemo(() => crosshair(r), [r]);
  return (
    <group position={position}>
      <lineSegments geometry={geometry}>
        <lineBasicMaterial color={COLOR} depthTest={false} transparent opacity={0.9} />
      </lineSegments>
    </group>
  );
}

/** Three axis-aligned line segments crossing at the local origin. */
function crosshair(r: number): BufferGeometry {
  const g = new BufferGeometry();
  // prettier-ignore
  const verts = [
    -r, 0, 0,  r, 0, 0,
    0, -r, 0,  0, r, 0,
    0, 0, -r,  0, 0, r,
  ];
  g.setAttribute("position", new Float32BufferAttribute(verts, 3));
  return g;
}
