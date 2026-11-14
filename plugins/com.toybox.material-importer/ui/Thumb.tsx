/**
 * A square preview of one image. Loads only once it scrolls into view; shows a
 * neutral box while loading and a "?" when there is no image or it cannot be read.
 * The loader owns the URL, so this never revokes it.
 */

import { useEffect, useRef, useState, type RefObject } from "react";
import type { ThumbLoader } from "./thumbs.js";

interface Props {
  loader: ThumbLoader;
  relPath: string | undefined;
  size: number;
  /** CSS box edge in px */
  box: number;
  title?: string;
}

type Shown = { relPath: string; state: "loaded"; url: string } | { relPath: string; state: "failed" };

export function Thumb({ loader, relPath, size, box, title }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const seen = useSeen(ref);
  const shown = useThumb(loader, relPath, size, seen);

  return (
    <div
      ref={ref}
      title={title}
      style={{
        width: box,
        height: box,
        flex: "none",
        borderRadius: "var(--radius-1)",
        background: "var(--bg-2)",
        overflow: "hidden",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        color: "var(--text-2)",
        fontSize: "var(--fs-sm)",
      }}
    >
      {shown?.state === "loaded" && (
        <img
          src={shown.url}
          alt=""
          draggable={false}
          style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }}
        />
      )}
      {(!relPath || shown?.state === "failed") && "?"}
    </div>
  );
}

/** True once the element has scrolled into view; the observer is dropped then. */
function useSeen(ref: RefObject<HTMLElement | null>): boolean {
  const [seen, setSeen] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (seen || !el) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        setSeen(true);
        observer.disconnect();
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref, seen]);
  return seen;
}

/** The loaded image (or failure) for `relPath`; ignores a result for an earlier
 *  path or one that arrives after unmount. */
function useThumb(
  loader: ThumbLoader,
  relPath: string | undefined,
  size: number,
  enabled: boolean,
): Shown | undefined {
  const [shown, setShown] = useState<Shown>();
  useEffect(() => {
    if (!enabled || !relPath) return;
    let current = true;
    loader.load(relPath, size).then(
      (url) => current && setShown({ relPath, state: "loaded", url }),
      () => current && setShown({ relPath, state: "failed" }),
    );
    return () => {
      current = false;
    };
  }, [loader, relPath, size, enabled]);
  // A result for a different path is stale: show the neutral box until the new one arrives.
  return shown?.relPath === relPath ? shown : undefined;
}
