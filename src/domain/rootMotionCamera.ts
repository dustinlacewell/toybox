import { Vector3, type Object3D } from "three";

/** Follow ground-plane root travel by translating camera and orbit target together.
 * Never resample the animation or overwrite the user's orbit, pan, or zoom.
 */
export function rootMotionCamera(camera:{position:Vector3},controls:{target:Vector3},root:Object3D,anchor:Vector3) {
  const offset=new Vector3(),next=new Vector3(),delta=new Vector3();
  return {offset,update(enabled:boolean){
    if(enabled){root.getWorldPosition(next);next.sub(anchor);next.y=0;}
    else next.set(0,0,0);
    delta.copy(next).sub(offset);
    camera.position.add(delta);controls.target.add(delta);offset.copy(next);
  }};
}
