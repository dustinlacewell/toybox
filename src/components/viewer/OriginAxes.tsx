/**
 * RGB axis arrows at the world origin: X=red, Y=green, Z=blue (the standard
 * convention). Built from three.js ArrowHelpers grouped together; the arrow
 * length scales with the model so it reads at any size. Disposed on unmount.
 */

import { useEffect, useMemo } from "react";
import { ArrowHelper, Group, type Material, Vector3 } from "three";

interface Props {
  /** Reference length for the arrows (typically the model's bounding radius). */
  length: number;
}

const AXES: { dir: Vector3; color: number }[] = [
  { dir: new Vector3(1, 0, 0), color: 0xe5484d }, // X red
  { dir: new Vector3(0, 1, 0), color: 0x46a758 }, // Y green
  { dir: new Vector3(0, 0, 1), color: 0x4a90e2 }, // Z blue
];

export function OriginAxes({ length }: Props) {
  const group = useMemo(() => buildArrows(length), [length]);
  useEffect(() => () => disposeArrows(group), [group]);
  return <primitive object={group} />;
}

function buildArrows(length: number): Group {
  const origin = new Vector3(0, 0, 0);
  const headLength = length * 0.18;
  const headWidth = headLength * 0.6;

  const group = new Group();
  for (const { dir, color } of AXES) {
    group.add(new ArrowHelper(dir, origin, length, color, headLength, headWidth));
  }
  return group;
}

function disposeArrows(group: Group): void {
  group.traverse((obj) => {
    const helper = obj as ArrowHelper;
    helper.line?.geometry?.dispose?.();
    helper.cone?.geometry?.dispose?.();
    (helper.line?.material as Material | undefined)?.dispose?.();
    (helper.cone?.material as Material | undefined)?.dispose?.();
  });
}
