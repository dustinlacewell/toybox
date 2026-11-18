/**
 * The thumb loader for one scanned source folder. Made when a source is set,
 * disposed when the source changes or the panel unmounts. Gives `undefined` while
 * there is no source, so a caller never gets a loader for an earlier folder.
 */

import { useEffect, useRef, useState } from "react";
import type { FsApi } from "@ldlework/toybox-sdk";
import { createThumbLoader, type ThumbLoader } from "./thumbs.js";

type Made = { source: string; loader: ThumbLoader };

export function useThumbLoader(
  fs: Pick<FsApi, "readImageThumb">,
  source: string | undefined,
): ThumbLoader | undefined {
  // The fs handle is read when a source is set; a new handle alone does not remake the loader.
  const fsRef = useRef(fs);
  fsRef.current = fs;

  const [made, setMade] = useState<Made>();
  useEffect(() => {
    if (!source) {
      setMade(undefined);
      return;
    }
    const loader = createThumbLoader(fsRef.current, source);
    setMade({ source, loader });
    return () => loader.dispose();
  }, [source]);

  return made && made.source === source ? made.loader : undefined;
}
