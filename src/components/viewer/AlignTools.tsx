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

import type { Align, Axis } from "../../services/tauriApi";
import "./AlignTools.css";

interface Props {
  onAlign: (axis: Axis, align: Align) => Promise<void>;
  busy: boolean;
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

export function AlignTools({ onAlign, busy }: Props) {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <div className="align">
      <button className="align__title" onClick={() => setCollapsed((c) => !c)}>
        Origin alignment {collapsed ? "▸" : "▾"}
      </button>
      {!collapsed && (
        <div className="align__grid">
          {AXES.map(({ axis, label }) => (
            <div className="align__row" key={axis}>
              <span className={`align__axis align__axis--${axis}`}>{label}</span>
              {ALIGNS.map(({ align, label }) => (
                <button
                  key={align}
                  className="align__btn"
                  disabled={busy}
                  onClick={() => void onAlign(axis, align)}
                  title={`Snap ${label.toLowerCase()} of ${label === "Center" ? "" : "the "}${axis.toUpperCase()} bounds to the origin`}
                >
                  {label}
                </button>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
