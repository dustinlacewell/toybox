/**
 * The interactive 3D part of a material viewer: for a given `MaterialState`, the
 * toolbar and the preview canvas once ready, a spinner while loading, or the
 * error. It owns the shape and tiling choice and fills its parent. Loading and
 * disposal belong to useMaterial.
 */

import { useState, type ReactNode } from "react";

import type { PreviewShape } from "../../services/materialPreview";
import { Spinner } from "@ldlework/toybox-sdk/ui";
import "../ViewerFrame.css";
import { MaterialPreview } from "./MaterialPreview";
import { MaterialToolbar, type Tiling } from "./MaterialToolbar";
import type { MaterialState } from "./useMaterial";
import "./MaterialStudio.css";
import { usePreviewPreferences } from "../../state/previewPreferences";
import type { PreviewBrightness } from "../../domain/previewBrightness";

export function MaterialStudio({ state, controls, active=true }: { state: MaterialState; controls?: ReactNode; active?:boolean }) {
  const preferredShape=usePreviewPreferences(state=>state.materialStyle);
  const [chosenShape,setShape]=useState<PreviewShape>();
  const shape=chosenShape??preferredShape;
  const [tiling, setTiling] = useState<Tiling>(1);
  const [resetKey,setResetKey]=useState(0);
  const [brightness,setBrightness]=useState<PreviewBrightness>("dim");

  return (
    <div className="matstudio" hidden={!active}>
      {state.kind === "ready" && (
        <>
          <MaterialToolbar shape={shape} onShape={setShape} tiling={tiling} onTiling={setTiling} trailing={controls} onResetCamera={()=>setResetKey(value=>value+1)} brightness={brightness} onBrightness={setBrightness}/>
          <MaterialPreview material={state.material} shape={shape} tiling={tiling} resetKey={resetKey} brightness={brightness} active={active}/>
        </>
      )}
      {state.kind === "loading" && (
        <div className="viewer__loading">
          <Spinner />
        </div>
      )}
      {state.kind === "error" && <div className="viewer__error">Failed to load: {state.message}</div>}
    </div>
  );
}
