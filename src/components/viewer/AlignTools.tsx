/**
 * Origin-alignment tools: per-axis, snap the model's bounding-box min / center /
 * max onto the local origin (the point a game engine pivots around). Used to fix
 * assets whose geometry was authored far from their origin. Each click bakes a
 * correction into the source .gltf and the viewer reloads to show it.
 *
 * Labels per axis: Min / Center / Max (the "front / middle / back" of the box
 * along that axis).
 */

import { useState } from "react";
import { Button } from "@ldlework/toybox-sdk/ui";

import type { Align, Axis } from "../../services/tauriApi";
import "./AlignTools.css";

interface Props {
  onAlign: (axis: Axis, align: Align) => Promise<void>;
  busy: boolean;
  error?: string;
}

const AXES: { axis: Axis; label: string }[] = [
  { axis: "x", label: "X" },
  { axis: "y", label: "Y" },
  { axis: "z", label: "Z" },
];

const ALIGNS: { align: Align; label: string }[] = [
  { align: "min", label: "Min" },
  { align: "center", label: "Center" },
  { align: "max", label: "Max" },
];

export function AlignTools({ onAlign, busy, error }: Props) {
  const [collapsed, setCollapsed] = useState(true);

  return (
    <div className="align">
      <Button variant="ghost" className="align__title" aria-expanded={!collapsed} onClick={() => setCollapsed((c) => !c)}>
        Origin alignment {collapsed ? "▸" : "▾"}
      </Button>
      {!collapsed && (
        <div className="align__grid">
          {AXES.map(({ axis, label }) => (
            <div className="align__row" key={axis}>
              <span className={`align__axis align__axis--${axis}`}>{label}</span>
              {ALIGNS.map(({ align, label }) => (
                <Button
                  key={align}
                  className="align__btn"
                  disabled={busy}
                  aria-label={`Align ${axis.toUpperCase()} ${align} to origin`}
                  onClick={() => void onAlign(axis, align)}
                  title={`Snap ${label.toLowerCase()} of ${label === "Center" ? "" : "the "}${axis.toUpperCase()} bounds to the origin`}
                >
                  {label}
                </Button>
              ))}
            </div>
          ))}
        </div>
      )}
      {error&&<p className="align__error" role="alert">{error}</p>}
    </div>
  );
}
