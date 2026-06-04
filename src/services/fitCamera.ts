/**
 * Position a perspective camera so an object's bounding sphere fills the frame
 * at a fixed, deterministic angle. Used by the thumbnail renderer so every
 * thumbnail is framed identically (and re-runs are reproducible).
 */

import { Box3, Object3D, PerspectiveCamera, Sphere, Vector3 } from "three";

/** Fixed view direction (elevated 3/4 angle), normalized. */
const VIEW_DIR = new Vector3(1, 0.7, 1).normalize();

export function fitCameraToObject(
  camera: PerspectiveCamera,
  object: Object3D,
  margin = 1.2,
): void {
  const box = new Box3().setFromObject(object);
  const sphere = box.getBoundingSphere(new Sphere());
  const radius = Math.max(sphere.radius, 1e-3);

  const fov = (camera.fov * Math.PI) / 180;
  const distance = (radius * margin) / Math.sin(fov / 2);

  const eye = VIEW_DIR.clone().multiplyScalar(distance).add(sphere.center);
  camera.position.copy(eye);
  // Near/far must scale with the object: library assets range from km-scale
  // environments down to sub-millimetre props, so a fixed near floor (e.g.
  // 0.01) would clip a tiny object that sits closer than that floor. Derive the
  // planes from the camera distance instead.
  camera.near = Math.max(distance - radius * 2, distance * 0.01);
  camera.far = distance + radius * 4;
  camera.lookAt(sphere.center);
  camera.updateProjectionMatrix();
}
