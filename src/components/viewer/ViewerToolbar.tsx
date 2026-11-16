/**
 * Floating toolbar of icon toggles for the viewer's reference widgets. Renders
 * one button per WIDGETS entry; active widgets are highlighted.
 */

import { Target, RotateCcw } from "lucide-react";
import type { ReactNode } from "react";
import { Button, IconButton } from "@ldlework/toybox-sdk/ui";

import { WIDGETS, type WidgetKey, type WidgetVisibility } from "./widgets";
import type { OrbitTarget } from "./ViewerCamera";
import "./ViewerToolbar.css";

interface Props {
  visibility: WidgetVisibility;
  onToggle: (key: keyof WidgetVisibility) => void;
  orbitTarget: OrbitTarget;
  onCycleOrbitTarget: () => void;
  actions?: ReactNode;
  available?:readonly WidgetKey[];
  onResetCamera?:()=>void;
}

export function ViewerToolbar({
  visibility,
  onToggle,
  orbitTarget,
  onCycleOrbitTarget,
  actions,
  available,
  onResetCamera,
}: Props) {
  return (
    <div className="vtoolbar" role="toolbar" aria-label="Scene controls">
      {WIDGETS.filter(widget=>!available||available.includes(widget.key)).map(({ key, label, icon }) => <IconButton key={key} icon={icon} label={label} active={visibility[key]} aria-pressed={visibility[key]} onClick={()=>onToggle(key)}/>)}
      {actions}

      <span className="vtoolbar__sep" />

      <Button variant="ghost"
        className="vtoolbar__target"
        onClick={onCycleOrbitTarget}
        title={`Orbit around: ${orbitTarget === "geometry" ? "model" : "world origin"}`}
        aria-label={`Orbit around: ${orbitTarget === "geometry" ? "model" : "world origin"}`}
      >
        <Target size={18} strokeWidth={2} />
        <span className="vtoolbar__tag">
          {orbitTarget === "geometry" ? "Model" : "Origin"}
        </span>
      </Button>
      {onResetCamera&&<IconButton icon={RotateCcw} label="Reset camera" onClick={onResetCamera}/>}
    </div>
  );
}
