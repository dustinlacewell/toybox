/**
 * Floating toolbar of icon toggles for the viewer's reference widgets. Renders
 * one button per WIDGETS entry; active widgets are highlighted.
 */

import { Target } from "lucide-react";

import { WIDGETS, type WidgetVisibility } from "./widgets";
import type { OrbitTarget } from "./ViewerCamera";
import "./ViewerToolbar.css";

interface Props {
  visibility: WidgetVisibility;
  onToggle: (key: keyof WidgetVisibility) => void;
  orbitTarget: OrbitTarget;
  onCycleOrbitTarget: () => void;
}

export function ViewerToolbar({
  visibility,
  onToggle,
  orbitTarget,
  onCycleOrbitTarget,
}: Props) {
  return (
    <div className="vtoolbar">
      {WIDGETS.map(({ key, label, icon: Icon }) => (
        <button
          key={key}
          className={`vtoolbar__btn ${visibility[key] ? "is-active" : ""}`}
          onClick={() => onToggle(key)}
          title={label}
          aria-pressed={visibility[key]}
        >
          <Icon size={18} strokeWidth={2} />
        </button>
      ))}

      <span className="vtoolbar__sep" />

      <button
        className="vtoolbar__btn"
        onClick={onCycleOrbitTarget}
        title={`Orbit around: ${orbitTarget === "geometry" ? "model" : "world origin"}`}
      >
        <Target size={18} strokeWidth={2} />
        <span className="vtoolbar__tag">
          {orbitTarget === "geometry" ? "Model" : "Origin"}
        </span>
      </button>
    </div>
  );
}
