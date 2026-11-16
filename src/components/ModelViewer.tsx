/**
 * Interactive 3D viewer for a single model asset, shown inside ViewerFrame.
 * Mounts an r3f <Canvas> only while an asset is open (so the grid never pays
 * for a live GL context). The model is
 * shown at its authored glTF origin (not recentered) over an infinite origin
 * grid with RGB axis arrows, so its true pivot is visible. The camera auto-frames
 * the model; the grid/axes stay origin-anchored. GPU resources are disposed when
 * the viewer closes or switches asset.
 */

import { useRef, useState } from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import { Mesh, Vector3, type AnimationClip, type Group } from "three";

import type { ModelAsset } from "../domain/catalog";
import { recenterAsset, type Align, type Axis } from "../services/tauriApi";

import { Spinner, Checkbox, NavigationItem, PropertyRow } from "@ldlework/toybox-sdk/ui";
import { variantInfo } from "@ldlework/toybox-sdk";
import { useScene } from "./viewer/useScene";
import { useCompatiblePreviews } from "./viewer/useCompatiblePreviews";
import { PreviewPicker } from "./viewer/PreviewPicker";
import { RigFacts } from "./viewer/RigFacts";
import { VariantSwitch } from "./VariantSwitch";
import { ViewerFrame } from "./ViewerFrame";
import { ViewerCamera } from "./viewer/ViewerCamera";
import { useViewerSceneControls, ViewerSceneControls, ViewerReferenceWidgets } from "./viewer/ViewerSceneControls";
import { ViewerSceneEnvironment } from "./viewer/ViewerSceneEnvironment";
import { AlignTools } from "./viewer/AlignTools";
import { AnimationBar } from "./viewer/AnimationBar";
import { AnimationDriver } from "./viewer/AnimationDriver";
import { useAnimation } from "./viewer/useAnimation";
import { type ModelExtent } from "./viewer/modelExtent";

interface Props {
  onExport?: () => void;
  asset: ModelAsset;
  onClose: () => void;
}

/** Stable empty-clip reference so useAnimation's deps don't churn when idle. */
const EMPTY_CLIPS: AnimationClip[] = [];

export function ModelViewer({ asset, onClose, onExport }: Props) {
  const [reloadNonce, setReloadNonce] = useState(0);
  const [variant, setVariant] = useState(0);
  const [byDistance, setByDistance] = useState(false);
  const [inPlace, setInPlace] = useState(true);
  const variants = [asset, ...(asset.variants ?? []).filter((a): a is ModelAsset => a.kind === "model")];
  const active = variants[variant] ?? asset;
  const preview = useCompatiblePreviews(active, asset);
  const state = useScene(active, reloadNonce, preview.target,!!preview.selected?.retargeted);
  const viewControls=useViewerSceneControls();
  const [aligning, setAligning] = useState(false);
  const [alignmentError,setAlignmentError]=useState<string>();

  const ready = state.kind === "ready" ? state : null;
  const anim = useAnimation(ready?.group ?? null, ready?.clips ?? EMPTY_CLIPS, { inPlace });

  const onAlign = async (axis: Axis, align: Align) => {
    setAligning(true);setAlignmentError(undefined);
    try {
      await recenterAsset(asset.id, axis, align);
      // The source .gltf changed; reload to show the corrected placement.
      setReloadNonce((n) => n + 1);
    } catch(error) {
      setAlignmentError(String(error));
    } finally {
      setAligning(false);
    }
  };

  return (
    <ViewerFrame asset={asset} onClose={onClose} onExport={onExport} footer={<AnimationBar controls={anim} />} sidebarTabs={[
      {value:"animations",label:"Animations",content:<><PreviewPicker kind="animation" preview={preview} /><RigFacts asset={active} />{preview.selected&&<Checkbox label="In place" checked={inPlace} onChange={setInPlace}/>}</>},
      {value:"materials",label:"Materials",content:ready&&materialNames(ready.group).map(name=><PropertyRow key={name} label={name} value="Embedded"/>)},
      {value:"levels",label:"Detail levels",content:variants.map((level,i)=><NavigationItem key={level.id} label={variantInfo(level.fileName,"model").label} active={variant===i} onPress={()=>{setByDistance(false);setVariant(i);}}/>)}
    ]} detailsExtra={<>
      <div className="viewer-panel-section">{variantInfo(active.fileName, "model").label} detail</div>
      {ready && <PropertyRow label="Triangles" value={triangleCount(ready.group).toLocaleString()} />}
    </>}>
      {state.kind === "ready" && (
        <>
          <ViewerSceneControls controls={viewControls}>{variant === 0 && <AlignTools onAlign={onAlign} busy={aligning} error={alignmentError} />}</ViewerSceneControls>
          {variants.length > 1 && <div className="viewer__variants">
            <VariantSwitch variants={variants} selected={variant} onSelect={(i) => { setByDistance(false); setVariant(i); }} />
            <Checkbox checked={byDistance} onChange={setByDistance} label="By distance" />
          </div>}
        </>
      )}
      <Canvas camera={{ position: [3, 2, 4], fov: 45 }} dpr={[1, 2]}>
        <ViewerSceneEnvironment radius={ready?.extent.radius} brightness={viewControls.brightness}/>
        {state.kind === "ready" && (
          <>
            {/* Model at its authored local origin (no recentering) so its
                true placement relative to the origin is visible. */}
            <primitive object={state.group} />
            {byDistance && <DistanceVariants extent={state.extent} count={variants.length} selected={variant} onSelect={setVariant} />}

            {/* Reference widgets — toggled from the viewer toolbar. */}
            <ViewerReferenceWidgets extent={state.extent} visibility={viewControls.visibility}/>

            <ViewerCamera
              center={state.extent.center}
              radius={state.extent.radius}
              target={viewControls.orbitTarget}
              frameKey={asset.id}
              resetKey={viewControls.resetKey}
            />
            <AnimationDriver tick={anim.tick} />
          </>
        )}
      </Canvas>

      {state.kind === "loading" && (
        <div className="viewer__loading">
          <Spinner />
        </div>
      )}
      {state.kind === "error" && (
        <div className="viewer__error">Failed to load: {state.message}</div>
      )}
      {ready?.loading&&<div className="viewer__preview-status" role="status">Loading animation…</div>}
      {ready?.previewError&&<div className="viewer__preview-status" role="alert">{ready.previewError}</div>}
    </ViewerFrame>
  );
}

function triangleCount(group: Group): number {
  let count = 0;
  group.traverse((node) => {
    if (node instanceof Mesh) count += (node.geometry.index?.count ?? node.geometry.attributes.position?.count ?? 0) / 3;
  });
  return Math.round(count);
}

function materialNames(group: Group): string[] {
  const names = new Set<string>();
  group.traverse((node) => { if (node instanceof Mesh) for (const material of Array.isArray(node.material) ? node.material : [node.material]) names.add(material.name || "Unnamed material"); });
  return [...names];
}

function DistanceVariants({ extent, count, selected, onSelect }: { extent: ModelExtent; count: number; selected: number; onSelect: (i: number) => void }) {
  const reference = useRef({ center: new Vector3(...extent.center), radius: Math.max(extent.radius, 0.001) });
  useFrame(({ camera }) => {
    const distance = camera.position.distanceTo(reference.current.center) / reference.current.radius;
    const index = Math.min(count - 1, distance > 10 ? 2 : distance > 5 ? 1 : 0);
    if (index !== selected) onSelect(index);
  });
  return null;
}
