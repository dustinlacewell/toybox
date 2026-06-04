/**
 * Ticks the animation mixer each frame from inside the r3f render loop. Renders
 * nothing; it exists only to bridge `useFrame` (Canvas-only) to the animation
 * controls owned by the viewer.
 */

import { useFrame } from "@react-three/fiber";

interface Props {
  tick: (delta: number) => void;
}

export function AnimationDriver({ tick }: Props) {
  useFrame((_, delta) => tick(delta));
  return null;
}
