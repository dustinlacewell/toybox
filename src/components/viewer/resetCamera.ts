import type { PerspectiveCamera, Vector3 } from "three";

export interface OrbitCameraControls {target:Vector3;enableDamping:boolean;update:()=>unknown;}

/** Clear orbit inertia before restoring a camera pose, without remounting the scene. */
export function resetCamera(camera:PerspectiveCamera,controls:OrbitCameraControls,position:Vector3,target:Vector3) {
  const damping=controls.enableDamping;
  controls.enableDamping=false;controls.update();
  camera.position.copy(position);camera.up.set(0,1,0);camera.zoom=1;camera.updateProjectionMatrix();
  controls.target.copy(target);controls.update();controls.enableDamping=damping;
}
