import { useEffect, useMemo, useState } from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import type { Group } from "three";
import { IconButton, PropertyRow, Segmented, Spinner } from "@ldlework/toybox-sdk/ui";
import { Bone } from "lucide-react";
import type { AnimationAsset } from "../domain/catalog";
import { createRigGuide, hasVisibleGeometry } from "../services/rigGuide";
import { disposeScene } from "../services/disposeScene";
import { rootMotion } from "../domain/animationMotion";
import { ViewerFrame } from "./ViewerFrame";
import { AnimationBar } from "./viewer/AnimationBar";
import { AnimationDriver } from "./viewer/AnimationDriver";
import { useViewerSceneControls, ViewerSceneControls, ViewerReferenceWidgets } from "./viewer/ViewerSceneControls";
import { ViewerSceneEnvironment } from "./viewer/ViewerSceneEnvironment";
import { ViewerCamera } from "./viewer/ViewerCamera";
import { useAnimation } from "./viewer/useAnimation";
import { useScene } from "./viewer/useScene";
import { useCompatiblePreviews } from "./viewer/useCompatiblePreviews";
import { PreviewPicker } from "./viewer/PreviewPicker";
import { RigFacts } from "./viewer/RigFacts";

const EMPTY_CLIPS: never[] = [];
const ANIMATION_WIDGETS=["grid","axes","geometryCenter"] as const;
interface Props { asset: AnimationAsset; onClose: () => void; onExport?: () => void; }
export function AnimationViewer({ asset, onClose, onExport }: Props) {
  const preview = useCompatiblePreviews(asset);
  const state = useScene(asset,0,preview.target,!!preview.selected?.retargeted);
  const ready = state.kind === "ready" ? state : null;
  const [inPlace, setInPlace] = useState(false);
  const [speed, setSpeed] = useState("1");
  const [showRig, setShowRig] = useState(false);
  const viewControls=useViewerSceneControls(ANIMATION_WIDGETS);
  const anim = useAnimation(ready?.group ?? null, ready?.clips ?? EMPTY_CLIPS, { speed: Number(speed) });
  const motion = useMemo(() => ready?.clips[0] ? rootMotion(ready.group, ready.clips[0]) : null, [ready]);
  return <ViewerFrame asset={asset} onClose={onClose} onExport={onExport}
    sidebarLabel="Animation"
    footer={<AnimationBar controls={anim} />} sidebar={<>
      <h2 className="viewer-panel-title">{asset.clip.name}</h2><p className="viewer-panel-subtitle">{asset.clip.sourceName}</p>
      <PreviewPicker kind="model" preview={preview} />
      {ready?.previewMethod && <p className="viewer-panel-subtitle">{ready.previewMethod}</p>}
      <RigFacts asset={asset} />
      <PropertyRow label="Length" value={`${(anim.duration || asset.clip.duration).toFixed(2)} s`} />
      <PropertyRow label="Rig" value={asset.clip.rig} />
      <PropertyRow label="Animated nodes" value={asset.clip.targetNodes.length} />
      <PropertyRow label="Root displacement" value={motion ? `${motion.displacement.length().toFixed(2)} m` : "No translation track"} />
      <PropertyRow label="Playback" value="Loops" />
      <div className="viewer-panel-section">Animated nodes</div>{asset.clip.targetNodes.map((name) => <PropertyRow key={name} label={name} value="" />)}
    </>}>
    {ready&&<ViewerSceneControls controls={viewControls} actions={<IconButton icon={Bone} label="Skeleton" active={showRig} aria-pressed={showRig} onClick={()=>setShowRig(value=>!value)}/>}/>}
    <Canvas camera={{ position: [3, 2, 4], fov: 45 }} dpr={[1, 2]}>
      <ViewerSceneEnvironment radius={ready?.extent.radius} brightness={viewControls.brightness}/>
      {ready && <><primitive object={ready.group} /><ViewerReferenceWidgets extent={ready.extent} visibility={viewControls.visibility}/>
        {(showRig || !hasVisibleGeometry(ready.group)) && <RigGuide root={ready.group} size={ready.extent.size} />}
        <ViewerCamera center={ready.extent.center} radius={ready.extent.radius} target={viewControls.orbitTarget} frameKey={`${asset.id}:${preview.value}`} resetKey={viewControls.resetKey} follow={motion?{node:motion.node,anchor:motion.anchor,enabled:inPlace}:undefined}/><AnimationDriver tick={anim.tick} />
      </>}
    </Canvas>
    {ready && <div className="viewer__variants"><Segmented label="Root motion preview" value={inPlace ? "place" : "travel"} options={[{ value: "travel", label: "Travel" }, { value: "place", label: "In place" }]} onChange={(value) => setInPlace(value === "place")} />
      <Segmented label="Playback speed" value={speed} options={[{ value: "0.5", label: "½×" }, { value: "1", label: "1×" }, { value: "2", label: "2×" }]} onChange={setSpeed} />
    </div>}
    {state.kind === "loading" && <div className="viewer__loading"><Spinner /></div>}{state.kind === "error" && <div className="viewer__error">{state.message}</div>}
  </ViewerFrame>;
}

function RigGuide({ root, size }: { root: Group; size: number }) {
  const guide = useMemo(() => createRigGuide(root, size), [root, size]);
  useFrame(() => guide.update());
  useEffect(() => () => disposeScene(guide.object), [guide]);
  return <primitive object={guide.object} />;
}
