/**
 * One of a material's texture files shown flat and whole, as stored, filling
 * the stage. The URL comes from the asset protocol.
 */

import { useEffect, useState } from "react";

import { assetUrl } from "../../services/assetUrl";

interface Props {
  file: string;
}

type UrlState = { kind: "loading" } | { kind: "ready"; url: string } | { kind: "error" };

export function MapImage({ file }: Props) {
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
  return (
    <img
      className="matview__image"
      src={state.url}
      alt={file}
      onError={() => setState({ kind: "error" })}
    />
  );
}
