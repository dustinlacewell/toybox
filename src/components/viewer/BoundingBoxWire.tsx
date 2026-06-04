/**
 * Wireframe box around the geometry's world-space bounds, so its extent and any
 * offset from the origin are obvious at a glance. Built from three's Box3Helper;
 * disposed on unmount.
 */

import { useEffect, useMemo } from "react";
import { Box3, Box3Helper, Color, type Material, Vector3 } from "three";

interface Props {
  min: [number, number, number];
  max: [number, number, number];
}

const COLOR = "#6b7484"; // muted grey — structural, not an attention marker

export function BoundingBoxWire({ min, max }: Props) {
  const helper = useMemo(() => {
    const box = new Box3(new Vector3(...min), new Vector3(...max));
    return new Box3Helper(box, new Color(COLOR));
  }, [min, max]);

  useEffect(() => {
    return () => {
      helper.geometry.dispose();
      (helper.material as Material).dispose();
    };
  }, [helper]);

  return <primitive object={helper} />;
}
