/**
 * Measure a loaded model's world-space extent and key reference points. The
 * viewer surfaces these so a user can spot mispositioned assets (geometry far
 * from the origin/pivot) and, with the upcoming transform tools, correct them.
 *
 * The asset library spans many orders of magnitude (km environments to sub-mm
 * props), so grid/axis/marker sizes derive from the model rather than fixed
 * constants.
 */

import { Box3, type Object3D, Sphere, Vector3 } from "three";

export interface ModelExtent {
  /** Largest bounding-box dimension (world units). */
  size: number;
  /** Bounding-sphere radius (world units). */
  radius: number;
  /** Bounding-box center in world space — where the mesh actually sits. */
  center: [number, number, number];
  /** Bounding-box min corner (lowest point); base = center with y at min.y. */
  min: [number, number, number];
  /** Bounding-box max corner. */
  max: [number, number, number];
}

export function measureExtent(object: Object3D): ModelExtent {
  const box = new Box3().setFromObject(object);
  const size = box.getSize(new Vector3());
  const center = box.getCenter(new Vector3());
  const radius = box.getBoundingSphere(new Sphere()).radius;
  return {
    size: Math.max(size.x, size.y, size.z, 1e-6),
    radius: Math.max(radius, 1e-6),
    center: [center.x, center.y, center.z],
    min: [box.min.x, box.min.y, box.min.z],
    max: [box.max.x, box.max.y, box.max.z],
  };
}

/**
 * A "nice" grid cell step for a model of the given extent: the power of ten at or
 * below ~1/10 of the model size, so a model spans roughly 10–100 cells.
 */
export function niceCellSize(modelSize: number): number {
  const target = modelSize / 10;
  return Math.pow(10, Math.floor(Math.log10(target)));
}
