/**
 * Owns the viewer camera framing and orbit target. On a new model it frames the
 * geometry once (distance from the model radius, fixed 3/4 angle); the orbit
 * target follows the user's toggle between the geometry center and the world
 * origin, so an offset model reveals itself (the origin/axes swing aside) while
 * still being inspectable.
 */

import { useEffect, useRef } from "react";
import { useThree } from "@react-three/fiber";
import { PerspectiveCamera, Vector3 } from "three";

export type OrbitTarget = "geometry" | "origin";

interface Props {
  /** Geometry bounding-box center (world space). */
  center: [number, number, number];
  /** Model bounding radius — drives framing distance. */
  radius: number;
  /** Which point the camera orbits. */
  target: OrbitTarget;
  /** Re-frame when this changes (new asset). */
  frameKey: string;
}

const VIEW_DIR = new Vector3(1, 0.7, 1).normalize();

export function ViewerCamera({ center, radius, target, frameKey }: Props) {
  const camera = useThree((s) => s.camera) as PerspectiveCamera;
  const controls = useThree((s) => s.controls) as
    | { target: Vector3; update: () => void }
    | null;

  const [cx, cy, cz] = center;

  // Latest center, read without re-triggering effects — so an in-place model
  // reload (which moves the center) does NOT yank the camera or orbit point.
  const centerRef = useRef<[number, number, number]>([cx, cy, cz]);
  centerRef.current = [cx, cy, cz];

  // Frame the geometry once per asset (keyed on frameKey, not center/radius, so
  // a same-asset reload after an alignment leaves the view untouched).
  const framedFor = useRef<string | null>(null);
  useEffect(() => {
    if (!controls || framedFor.current === frameKey) return;
    framedFor.current = frameKey;

    const [gx, gy, gz] = centerRef.current;
    const fov = (camera.fov * Math.PI) / 180;
    const distance = (radius * 1.4) / Math.sin(fov / 2);
    const centerVec = new Vector3(gx, gy, gz);
    camera.position.copy(VIEW_DIR.clone().multiplyScalar(distance).add(centerVec));
    // Generous frustum scaled to the model: the scene is just one model plus an
    // infinite grid, so the planes must clear far past it under free orbit/zoom.
    // Both scale with `radius` (keeping depth precision across the library's huge
    // size range — sub-mm props to km environments). The ~1:20000 near:far ratio
    // stays within 24-bit depth precision while clearing the grid's fade range.
    camera.near = radius * 0.05;
    camera.far = radius * 1000;
    camera.updateProjectionMatrix();
    controls.target.copy(centerVec);
    controls.update();
  }, [controls, frameKey, radius, camera]);

  // Move the orbit target only on an explicit toggle or asset change — never on
  // a silent reload. Reads the current center via the ref.
  useEffect(() => {
    if (!controls) return;
    const [gx, gy, gz] = centerRef.current;
    if (target === "origin") controls.target.set(0, 0, 0);
    else controls.target.set(gx, gy, gz);
    controls.update();
  }, [controls, target, frameKey]);

  return null;
}
