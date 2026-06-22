/**
 * Bottom status strip for the background thumbnail-generation job. This is
 * transient job status, not toolbar chrome — so it lives in its own fixed-height
 * bar at the foot of the library, out of the persistent control strip. The strip
 * is present only while work is pending or running; nothing here changes width as
 * the job ticks, so the surrounding layout never reflows.
 *
 *   [ control ] [ ===== progress ===== ]            done/total · current-asset
 *
 * The bar flexes to fill; the count is width-reserved (tabular numerals); the
 * current-asset name truncates inside its own region and can't push siblings.
 */

import { Button } from "@ldlework/toybox-sdk/ui";
import type { ThumbProgress as Progress } from "../services/thumbQueue";
import "./ThumbStatusBar.css";

interface Props {
  progress: Progress | null;
  pendingCount: number;
  onStart: () => void;
  onPause: () => void;
}

export function ThumbStatusBar({ progress, pendingCount, onStart, onPause }: Props) {
  const active = !!progress && (progress.running || progress.done < progress.total);
  // Idle with nothing pending: no job to report, so the strip is absent.
  if (!active && pendingCount === 0) return null;

  return (
    <div className="thumbbar">
      {active ? (
        <RunningStatus progress={progress!} onStart={onStart} onPause={onPause} />
      ) : (
        <Button onClick={onStart}>Generate thumbnails ({pendingCount})</Button>
      )}
    </div>
  );
}

function RunningStatus({
  progress,
  onStart,
  onPause,
}: {
  progress: Progress;
  onStart: () => void;
  onPause: () => void;
}) {
  const pct = progress.total ? Math.round((progress.done / progress.total) * 100) : 0;
  return (
    <>
      {progress.running ? (
        <Button variant="ghost" onClick={onPause}>
          Pause
        </Button>
      ) : (
        <Button variant="ghost" onClick={onStart}>
          Resume
        </Button>
      )}
      <div className="thumbbar__bar">
        <div className="thumbbar__fill" style={{ width: `${pct}%` }} />
      </div>
      <span className="thumbbar__count">
        {progress.done}/{progress.total}
      </span>
      {progress.currentName && (
        <span className="thumbbar__name" title={progress.currentName}>
          {progress.currentName}
        </span>
      )}
    </>
  );
}
