import { Color, DirectionalLight, HemisphereLight, PerspectiveCamera, Scene, WebGLRenderer } from "three";

/** Shared lighting and viewport setup for static and live asset previews. */
export function createScenePreviewStage(canvas:HTMLCanvasElement,preserveDrawingBuffer=false) {
  const renderer=new WebGLRenderer({canvas,antialias:true,alpha:true,preserveDrawingBuffer});
  renderer.setClearColor(new Color(0x14161a),1);
  const scene=new Scene();scene.add(new HemisphereLight(0xcfd8e3,0x2b2f38,.9));
  const key=new DirectionalLight(0xffffff,1.2);key.position.set(5,8,5);
  const fill=new DirectionalLight(0xffffff,.4);fill.position.set(-4,2,-3);scene.add(key,fill);
  const camera=new PerspectiveCamera(45,1,.01,1000);
  return {renderer,scene,camera};
}
