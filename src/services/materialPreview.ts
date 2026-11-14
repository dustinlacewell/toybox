/**
 * Unit-sized preview geometry wearing a material: the shape a material is
 * shown on in thumbnails (and later the viewer).
 */

import {
  BoxGeometry,
  BufferGeometry,
  Mesh,
  type MeshStandardMaterial,
  PlaneGeometry,
  SphereGeometry,
} from "three";

export type PreviewShape = "sphere" | "plane" | "cube";

const GEOMETRY: Record<PreviewShape, () => BufferGeometry> = {
  sphere: () => new SphereGeometry(0.5, 64, 48),
  plane: () => new PlaneGeometry(1, 1),
  cube: () => new BoxGeometry(1, 1, 1),
};

/** A mesh of the given shape (about 1 unit across) wearing `material`. Caller owns disposal. */
export function buildPreviewMesh(material: MeshStandardMaterial, shape: PreviewShape): Mesh {
  return new Mesh(GEOMETRY[shape](), material);
}
