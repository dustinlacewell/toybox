/**
 * Unit-sized preview geometry wearing a material: the shape a material is
 * shown on in thumbnails (and later the viewer).
 */

import {
  BoxGeometry,
  BufferGeometry,
  Mesh,
  type MeshStandardMaterial,
  SphereGeometry,
  PlaneGeometry,
  type Vector3,
} from "three";

export type PreviewShape = "sphere" | "cube" | "floor" | "wall";
export const isInfinitePreview=(shape:PreviewShape)=>shape==="floor"||shape==="wall";

// The edges stay beyond the fog horizon, even at a recenter boundary. Smaller
// triangles and UVs centered on zero avoid precision shimmer on nearby texels.
const PLANE_SIZE=1024;
const RECENTER_STEP=256;
function infinitePlane() {
  const geometry=new PlaneGeometry(PLANE_SIZE,PLANE_SIZE,32,32),uv=geometry.attributes.uv;
  for(let index=0;index<uv.count;index++)uv.setXY(index,(uv.getX(index)-.5)*PLANE_SIZE,(uv.getY(index)-.5)*PLANE_SIZE);
  return geometry;
}

/** Keep the surface nearby while preserving texture coordinates in world space. */
export function recenterPreviewPlane(mesh:Mesh,shape:PreviewShape,camera:Vector3) {
  if(!isInfinitePreview(shape))return;
  const x=Math.round(camera.x/RECENTER_STEP)*RECENTER_STEP;
  const y=shape==="wall"?Math.round(camera.y/RECENTER_STEP)*RECENTER_STEP:0;
  const z=shape==="floor"?Math.round(camera.z/RECENTER_STEP)*RECENTER_STEP:0;
  if(mesh.position.x===x&&mesh.position.y===y&&mesh.position.z===z)return;
  mesh.position.set(x,y,z);
  const {position,uv}=mesh.geometry.attributes;
  for(let index=0;index<uv.count;index++) {
    uv.setXY(index,position.getX(index)+x,shape==="floor"?-position.getZ(index)-z:position.getY(index)+y);
  }
  uv.needsUpdate=true;
}

const GEOMETRY: Record<PreviewShape, () => BufferGeometry> = {
  sphere: () => new SphereGeometry(0.5, 64, 48),
  cube: () => new BoxGeometry(1, 1, 1),
  floor: () => infinitePlane().rotateX(-Math.PI/2),
  wall: () => infinitePlane(),
};

/** A mesh of the given shape (about 1 unit across) wearing `material`. Caller owns disposal. */
export function buildPreviewMesh(material: MeshStandardMaterial, shape: PreviewShape): Mesh {
  return new Mesh(GEOMETRY[shape](), material);
}
