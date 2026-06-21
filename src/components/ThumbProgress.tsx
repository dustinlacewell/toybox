/**
 * Thumbnail generation progress, shown in the toolbar while work is pending or
 * running. Pending: a "Generate thumbnails" button. Running: a progress bar
 * with the current asset name and a pause button. Renders nothing once every
 * thumbnail is ready — regeneration is driven from the toolbar's icon strip.
 */

import { Button } from "@ldlework/toybox-sdk/ui";
import type { ThumbProgress as Progress } from "../services/thumbQueue";
import "./ThumbProgress.css";

interface Props {
  progress: Progress | null;
  pendingCount: number;
  onStart: () => void;
  onPause: () => void;
}

export function ThumbProgress({ progress, pendingCount, onStart, onPause }: Props) {
  if (!progress || (!progress.running && progress.done >= progress.total)) {
    if (pendingCount === 0) return null;
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
