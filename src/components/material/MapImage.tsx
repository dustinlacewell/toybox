/**
 * One of a material's texture files shown flat and whole, as stored, filling
 * the stage. The URL comes from the asset protocol.
 */

import { useEffect, useState } from "react";

import { assetUrl } from "../../services/assetUrl";
import { ImageViewport, type ImageChannel, type ImageScale } from "@ldlework/toybox-sdk/ui";

interface Props {
  file: string;
  channel?: ImageChannel;
  scale?: ImageScale;
}

type UrlState = { kind: "loading" } | { kind: "ready"; url: string } | { kind: "error" };

export function MapImage({ file, channel, scale }: Props) {
  const [state, setState] = useState<UrlState>({ kind: "loading" });

  useEffect(() => {
    let alive = true;
    setState({ kind: "loading" });
    assetUrl(file).then(
      (url) => alive && setState({ kind: "ready", url }),
      () => alive && setState({ kind: "error" }),
    );
    return () => {
      alive = false;
    };
  }, [file]);

  if (state.kind === "error") {
    return <div className="viewer__error">Failed to load</div>;
  }
  if (state.kind === "loading") return null;
  return <ImageViewport src={state.url} alt={file} channel={channel} scale={scale} />;
}
