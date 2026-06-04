/**
 * Thumbnail generation control + progress, shown in the toolbar. Idle: a
 * "Generate thumbnails" button. Running: a progress bar with the current asset
 * name and a pause button.
 */

import { Button } from "../ds/Button";
import type { ThumbProgress as Progress } from "../services/thumbQueue";
import "./ThumbProgress.css";

interface Props {
  progress: Progress | null;
  pendingCount: number;
  onStart: () => void;
  onPause: () => void;
  onRegenerate: () => void;
}

export function ThumbProgress({
  progress,
  pendingCount,
  onStart,
  onPause,
  onRegenerate,
}: Props) {
  if (!progress || (!progress.running && progress.done >= progress.total)) {
    if (pendingCount === 0) {
      return (
        <div className="thumbp">
          <span className="thumbp__done">Thumbnails ready</span>
          <Button variant="ghost" onClick={onRegenerate}>
            Regenerate
          </Button>
        </div>
      );
    }
    return <Button onClick={onStart}>Generate thumbnails ({pendingCount})</Button>;
  }

  const pct = progress.total ? Math.round((progress.done / progress.total) * 100) : 0;
  return (
    <div className="thumbp">
      <div className="thumbp__bar">
        <div className="thumbp__fill" style={{ width: `${pct}%` }} />
      </div>
      <span className="thumbp__label">
        {progress.done}/{progress.total}
        {progress.currentName ? ` · ${progress.currentName}` : ""}
      </span>
      {progress.running ? (
        <Button variant="ghost" onClick={onPause}>
          Pause
        </Button>
      ) : (
        <Button variant="ghost" onClick={onStart}>
          Resume
        </Button>
      )}
    </div>
  );
}
