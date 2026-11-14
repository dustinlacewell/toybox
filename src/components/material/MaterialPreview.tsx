/**
 * The 3D preview of a loaded material: an r3f <Canvas> with orbit controls, the
 * studio lights and environment, and one unit-sized mesh of the chosen shape.
 * Switching shape builds a new mesh and disposes only the old geometry; the
 * material belongs to useMaterial. Tiling sets `repeat` on each of the
 * material's textures.
 */

import { useEffect, useMemo } from "react";
import { Canvas, useThree } from "@react-three/fiber";
import { OrbitControls } from "@react-three/drei";
import type { MeshStandardMaterial } from "three";

import { buildPreviewMesh, type PreviewShape } from "../../services/materialPreview";
import { studioEnvironment } from "../../services/studioEnvironment";
import { materialTextures } from "../../services/materialTextures";

interface Props {
  material: MeshStandardMaterial;
  shape: PreviewShape;
  tiling: number;
}

export function MaterialPreview({ material, shape, tiling }: Props) {
  return (
    <Canvas camera={{ position: [1.2, 0.8, 1.6], fov: 45 }} dpr={[1, 2]}>
      <ambientLight intensity={0.5} />
      <hemisphereLight args={["#cfd8e3", "#2b2f38", 0.8]} />
      <directionalLight position={[5, 8, 5]} intensity={1.2} />
      <directionalLight position={[-4, 2, -3]} intensity={0.4} />
      <StudioEnvironment />
      <PreviewMesh material={material} shape={shape} tiling={tiling} />
      <OrbitControls makeDefault enableDamping />
    </Canvas>
  );
}

/** Light the scene with the renderer's shared environment (never disposed here). */
function StudioEnvironment() {
  const gl = useThree((s) => s.gl);
  const scene = useThree((s) => s.scene);
  useEffect(() => {
    scene.environment = studioEnvironment(gl);
    return () => {
      scene.environment = null;
    };
  }, [gl, scene]);
  return null;
}

function PreviewMesh({ material, shape, tiling }: Props) {
  const mesh = useMemo(() => buildPreviewMesh(material, shape), [material, shape]);
  useEffect(() => () => mesh.geometry.dispose(), [mesh]);

  useEffect(() => {
    for (const texture of materialTextures(material)) texture.repeat.set(tiling, tiling);
  }, [material, tiling]);

  return <primitive object={mesh} />;
}
