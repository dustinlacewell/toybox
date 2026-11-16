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
  // Advance before orbit controls, camera following, and skeleton overlays.
  useFrame((_, delta) => tick(delta), -2);
  return null;
}
