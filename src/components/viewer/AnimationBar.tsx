/**
 * Transport bar for animation playback: play/pause, a scrub slider, and a clip
 * selector (shown only when an asset has more than one clip). Rendered over the
 * bottom of the viewer stage; reads/drives the controls from useAnimation.
 */

import { Pause, Play } from "lucide-react";

import type { AnimationControls } from "./useAnimation";
import "./AnimationBar.css";

export function AnimationBar({ controls }: { controls: AnimationControls }) {
  const { clips, currentIndex, playing, time, duration, toggle, seek, selectClip } =
    controls;
  if (clips.length === 0) return null;

  // Guard against transient non-finite values during clip swaps so the
  // controlled range input and the readout never render NaN.
  const dur = Number.isFinite(duration) && duration > 0 ? duration : 0;
  const t = Number.isFinite(time) ? Math.min(Math.max(time, 0), dur) : 0;

  return (
    <div className="animbar">
      <button
        className="animbar__play"
        onClick={toggle}
        title={playing ? "Pause" : "Play"}
      >
        {playing ? <Pause size={16} /> : <Play size={16} />}
      </button>

      <input
        className="animbar__scrub"
        type="range"
        min={0}
        max={dur || 1}
        step={0.001}
        value={t}
        onChange={(e) => seek(Number(e.currentTarget.value))}
      />
      <span className="animbar__time">
        {fmt(t)} / {fmt(dur)}
      </span>

      {clips.length > 1 && (
        <select
          className="animbar__clip"
          value={currentIndex}
          onChange={(e) => selectClip(Number(e.currentTarget.value))}
          title="Clip"
        >
          {clips.map((c, i) => (
            <option key={i} value={i}>
              {c.name || `Clip ${i + 1}`}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}

function fmt(seconds: number): string {
  return `${seconds.toFixed(1)}s`;
}
