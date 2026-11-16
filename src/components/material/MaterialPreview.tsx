/**
 * The 3D preview of a loaded material: an r3f <Canvas> with orbit controls, the
 * studio lights and environment, and one unit-sized mesh of the chosen shape.
 * Switching shape builds a new mesh and disposes only the old geometry; the
 * material belongs to useMaterial. Tiling sets `repeat` on each of the
 * material's textures.
 */

import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { Color, DoubleSide, Fog, Vector3, type PerspectiveCamera, type MeshStandardMaterial } from "three";
import { resetCamera, type OrbitCameraControls } from "../viewer/resetCamera";

import { buildPreviewMesh, isInfinitePreview, recenterPreviewPlane, type PreviewShape } from "../../services/materialPreview";
import { ViewerSceneEnvironment } from "../viewer/ViewerSceneEnvironment";
import { materialTextures } from "../../services/materialTextures";
import type { PreviewBrightness } from "../../domain/previewBrightness";

interface Props {
  material: MeshStandardMaterial;
  shape: PreviewShape;
  tiling: number;
  resetKey?:number;
  brightness?:PreviewBrightness;
  active?:boolean;
}

export function MaterialPreview({ material, shape, tiling, resetKey=0, brightness="normal", active=true }: Props) {
  return (
    <Canvas camera={{ position: [1.2, 0.8, 1.6], fov: 45 }} dpr={[1, 2]} frameloop={active?"always":"never"}>
      <ViewerSceneEnvironment brightness={brightness} radius={shape==="cube"?Math.sqrt(3)/2:.5} maxPolarAngle={shape==="floor"?Math.PI/2-.02:Math.PI} minAzimuthAngle={shape==="wall"?-Math.PI/2+.02:-Infinity} maxAzimuthAngle={shape==="wall"?Math.PI/2-.02:Infinity}/>
      <MaterialModeCameras shape={shape} resetKey={resetKey}/>
      {isInfinitePreview(shape)&&<InfiniteBackdrop/>}
      <PreviewMesh material={material} shape={shape} tiling={tiling} />
    </Canvas>
  );
}

interface CameraPose { position:Vector3; target:Vector3; zoom:number; }

function MaterialModeCameras({shape,resetKey}:{shape:PreviewShape;resetKey:number}) {
  const camera=useThree(state=>state.camera) as PerspectiveCamera;
  const controls=useThree(state=>state.controls) as OrbitCameraControls|null;
  const poses=useRef(new Map<PreviewShape,CameraPose>());
  const lastReset=useRef(resetKey);
  const capture=()=>{
    if(!controls)return;
    const pose=poses.current.get(shape);
    if(pose){pose.position.copy(camera.position);pose.target.copy(controls.target);pose.zoom=camera.zoom;}
    else poses.current.set(shape,{position:camera.position.clone(),target:controls.target.clone(),zoom:camera.zoom});
  };
  // Capture after OrbitControls updates, before a new mode can apply its limits.
  useFrame(capture);
  useLayoutEffect(()=>{
    if(!controls)return;
    const reset=lastReset.current!==resetKey;
    lastReset.current=resetKey;
    const pose=reset?undefined:poses.current.get(shape);
    resetCamera(camera,controls,pose?.position??new Vector3(1.2,.8,1.6),pose?.target??new Vector3());
    camera.zoom=pose?.zoom??1;camera.updateProjectionMatrix();
    controls.update();
    capture();
  },[camera,controls,shape,resetKey]);
  return null;
}

function PreviewMesh({ material, shape, tiling }: Props) {
  const infinite=isInfinitePreview(shape);
  const surface=useMemo(()=>{
    if(!infinite)return material;
    const clone=material.clone();clone.side=DoubleSide;return clone;
  },[material,infinite]);
  useEffect(()=>()=>{if(surface!==material)surface.dispose();},[surface,material]);
  const mesh = useMemo(() => buildPreviewMesh(surface, shape), [surface, shape]);
  useEffect(() => () => mesh.geometry.dispose(), [mesh]);
  useFrame(({camera})=>{
    recenterPreviewPlane(mesh,shape,camera.position);
  });

  useEffect(() => {
    for (const texture of materialTextures(material)) texture.repeat.set(tiling, tiling);
  }, [material, tiling]);

  return <primitive object={mesh} />;
}

function InfiniteBackdrop() {
  const scene=useThree(state=>state.scene),renderer=useThree(state=>state.gl);
  useEffect(()=>{
    const background=scene.background,fog=scene.fog;
    const color=getComputedStyle(renderer.domElement).getPropertyValue("--bg-0").trim()||"#14161a";
    scene.background=new Color(color);scene.fog=new Fog(color,50,200);
    return()=>{scene.background=background;scene.fog=fog;};
  },[scene,renderer]);
  return null;
}
