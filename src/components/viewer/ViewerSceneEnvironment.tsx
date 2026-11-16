import { useEffect } from "react";
import { useThree } from "@react-three/fiber";
import { OrbitControls } from "@react-three/drei";
import { MOUSE } from "three";
import { studioEnvironment } from "../../services/studioEnvironment";
import { PREVIEW_BRIGHTNESS, type PreviewBrightness } from "../../domain/previewBrightness";

/** Shared studio lighting and fixed-up navigation for every full 3D viewer. */
export function ViewerSceneEnvironment({radius=1,maxPolarAngle=Math.PI,minAzimuthAngle=-Infinity,maxAzimuthAngle=Infinity,brightness="normal"}:{radius?:number;maxPolarAngle?:number;minAzimuthAngle?:number;maxAzimuthAngle?:number;brightness?:PreviewBrightness}) {
  const scale=Math.max(Number.isFinite(radius)?radius:1,1e-6);
  const intensity=PREVIEW_BRIGHTNESS[brightness];
  const renderer=useThree(state=>state.gl),scene=useThree(state=>state.scene);
  useEffect(()=>{
    const previous=scene.environment;
    scene.environment=studioEnvironment(renderer);
    return()=>{scene.environment=previous;};
  },[renderer,scene]);
  useEffect(()=>{
    const previous=scene.environmentIntensity;
    scene.environmentIntensity=intensity;
    return()=>{scene.environmentIntensity=previous;};
  },[scene,intensity]);
  return <>
    <ambientLight intensity={.5*intensity}/>
    <hemisphereLight args={["#cfd8e3","#2b2f38",.8*intensity]}/>
    <directionalLight position={[5,8,5]} intensity={1.2*intensity}/>
    <directionalLight position={[-4,2,-3]} intensity={.4*intensity}/>
    <OrbitControls makeDefault enableDamping minDistance={scale*1.25} maxDistance={scale*20} maxPolarAngle={maxPolarAngle} minAzimuthAngle={minAzimuthAngle} maxAzimuthAngle={maxAzimuthAngle} mouseButtons={{LEFT:MOUSE.ROTATE,MIDDLE:MOUSE.PAN,RIGHT:MOUSE.DOLLY}}/>
  </>;
}
