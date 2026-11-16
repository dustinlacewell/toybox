import { useState, type ReactNode } from "react";
import { ViewerToolbar } from "./ViewerToolbar";
import { WIDGETS, defaultWidgetVisibility, type WidgetKey, type WidgetVisibility } from "./widgets";
import type { OrbitTarget } from "./ViewerCamera";
import type { ModelExtent } from "./modelExtent";
import { OriginGrid } from "./OriginGrid";
import { OriginAxes } from "./OriginAxes";
import { GeometryCenterMarker } from "./GeometryCenterMarker";
import { BoundingBoxWire } from "./BoundingBoxWire";
import { ViewportBrightnessControl } from "./ViewportBrightnessControl";
import type { PreviewBrightness } from "../../domain/previewBrightness";

/** Viewer state belongs to the app; the toolbar emits controlled UI intent. */
const ALL_WIDGET_KEYS=WIDGETS.map(widget=>widget.key);
export function useViewerSceneControls(available:readonly WidgetKey[]=ALL_WIDGET_KEYS) {
  const [visibility,setVisibility]=useState(defaultWidgetVisibility);
  const [orbitTarget,setOrbitTarget]=useState<OrbitTarget>("geometry");
  const [resetKey,setResetKey]=useState(0);
  const [brightness,setBrightness]=useState<PreviewBrightness>("dim");
  const enabled={...visibility};
  for(const key of ALL_WIDGET_KEYS)if(!available.includes(key))enabled[key]=false;
  return {visibility:enabled,orbitTarget,available,resetKey,brightness,setBrightness,
    resetCamera:()=>{setOrbitTarget("geometry");setResetKey(value=>value+1);},
    toggle:(key:WidgetKey)=>{if(available.includes(key))setVisibility(value=>({...value,[key]:!value[key]}));},
    cycleOrbitTarget:()=>setOrbitTarget(value=>value==="geometry"?"origin":"geometry"),
  };
}

export function ViewerSceneControls({controls,children,actions}:{controls:ReturnType<typeof useViewerSceneControls>;children?:ReactNode;actions?:ReactNode}) {
  return <div className="viewer-scene-controls"><ViewerToolbar visibility={controls.visibility} available={controls.available} onToggle={controls.toggle} orbitTarget={controls.orbitTarget} onCycleOrbitTarget={controls.cycleOrbitTarget} onResetCamera={controls.resetCamera} actions={actions}/><div className="viewer-scene-controls__right"><ViewportBrightnessControl value={controls.brightness} onChange={controls.setBrightness}/>{children}</div></div>;
}

/** Models and animations use the same reference geometry and visibility rules. */
export function ViewerReferenceWidgets({extent,visibility}:{extent:ModelExtent;visibility:WidgetVisibility}) {
  return <>
    {visibility.grid&&<OriginGrid modelSize={extent.size}/>}
    {visibility.axes&&<OriginAxes length={extent.radius}/>}
    {visibility.geometryCenter&&<GeometryCenterMarker position={extent.center} scale={extent.radius}/>}
    {visibility.boundingBox&&<BoundingBoxWire min={extent.min} max={extent.max}/>}
  </>;
}
