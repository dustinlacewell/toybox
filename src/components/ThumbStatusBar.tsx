/** Floating status for a running or paused thumbnail job. */

import { Button, Toast } from "@ldlework/toybox-sdk/ui";
import { useEffect, useState } from "react";
import type { ThumbProgress as Progress } from "../services/thumbQueue";
import "./ThumbStatusBar.css";

interface Props {
  progress: Progress | null;
  error?: string;
  onStart: () => void;
  onPause: () => void;
}

export function ThumbStatusBar({ progress, error, onStart, onPause }: Props) {
  const [dismissed, setDismissed] = useState<Progress | null>(null);
  const complete = !!progress && !progress.running && progress.total > 0 && progress.done >= progress.total;
  useEffect(() => {
    if (!complete || !progress) return;
    const timer = window.setTimeout(() => setDismissed(progress), 3000);
    return () => window.clearTimeout(timer);
  }, [complete, progress]);
  const active = !!progress && (progress.running || progress.done < progress.total || (complete && dismissed !== progress));
  if (error) return <Toast label="Thumbnail error"><p role="alert">{error}</p><Button onClick={onStart}>Retry thumbnails</Button></Toast>;
  if (!active) return null;

  return (
    <Toast label="Thumbnail progress"><div className="thumbbar">
      <strong>{complete?"Thumbnail update finished":progress!.running?"Updating thumbnails":"Thumbnails paused"}</strong>
      {complete?<span className="thumbbar__count">{progress!.done.toLocaleString()} processed</span>:<RunningStatus progress={progress!} onStart={onStart} onPause={onPause} />}
    </div></Toast>
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
      <div className="thumbbar__bar" role="progressbar" aria-label="Thumbnails generated" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}>
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
