/**
 * The interactive 3D part of a material viewer: for a given `MaterialState`, the
 * toolbar and the preview canvas once ready, a spinner while loading, or the
 * error. It owns the shape and tiling choice and fills its parent. Loading and
 * disposal belong to useMaterial.
 */

import { useState } from "react";

import type { PreviewShape } from "../../services/materialPreview";
import { Spinner } from "@ldlework/toybox-sdk/ui";
import "../ViewerFrame.css";
import { MaterialPreview } from "./MaterialPreview";
import { MaterialToolbar, type Tiling } from "./MaterialToolbar";
import type { MaterialState } from "./useMaterial";
import "./MaterialStudio.css";

export function MaterialStudio({ state }: { state: MaterialState }) {
  const [shape, setShape] = useState<PreviewShape>("sphere");
  const [tiling, setTiling] = useState<Tiling>(1);

  return (
    <div className="matstudio">
      {state.kind === "ready" && (
        <>
          <MaterialToolbar shape={shape} onShape={setShape} tiling={tiling} onTiling={setTiling} />
          <MaterialPreview material={state.material} shape={shape} tiling={tiling} />
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
