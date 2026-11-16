import type { ReactNode } from "react";
import { IconButton, Segmented } from "@ldlework/toybox-sdk/ui";
import { Box, Globe, Grid3x3, BrickWall, RotateCcw } from "lucide-react";
import "../viewer/ViewerToolbar.css";
import type { PreviewShape } from "../../services/materialPreview";
import type { PreviewBrightness } from "../../domain/previewBrightness";
import { ViewportBrightnessControl } from "../viewer/ViewportBrightnessControl";
export const TILING_VALUES = [1, 2, 4, 8] as const;
export type Tiling = (typeof TILING_VALUES)[number];
interface Props { shape: PreviewShape; onShape: (shape: PreviewShape) => void; tiling: Tiling; onTiling: (tiling: Tiling) => void; trailing?: ReactNode; onResetCamera:()=>void; brightness:PreviewBrightness; onBrightness:(value:PreviewBrightness)=>void; }
export function MaterialToolbar({ shape, onShape, tiling, onTiling, trailing, onResetCamera, brightness, onBrightness }: Props) {
  return <><div className="viewer-scene-controls"><div className="vtoolbar" role="toolbar" aria-label="Material preview controls">
    <IconButton icon={Globe} label="Sphere" active={shape==="sphere"} aria-pressed={shape==="sphere"} onClick={()=>onShape("sphere")}/>
    <IconButton icon={Box} label="Cube" active={shape==="cube"} aria-pressed={shape==="cube"} onClick={()=>onShape("cube")}/>
    <IconButton icon={Grid3x3} label="Infinite floor" active={shape==="floor"} aria-pressed={shape==="floor"} onClick={()=>onShape("floor")}/>
    <IconButton icon={BrickWall} label="Infinite wall" active={shape==="wall"} aria-pressed={shape==="wall"} onClick={()=>onShape("wall")}/>
    <span className="vtoolbar__sep"/>
    <IconButton icon={RotateCcw} label="Reset camera" onClick={onResetCamera}/>
  </div><div className="viewer-scene-controls__right"><ViewportBrightnessControl value={brightness} onChange={onBrightness}/></div></div><div className="mtoolbar">
    <Segmented value={String(tiling)} options={TILING_VALUES.map((n) => ({ value: String(n), label: `${n}×` }))} onChange={(value) => onTiling(Number(value) as Tiling)} />{trailing}
  </div></>;
}
