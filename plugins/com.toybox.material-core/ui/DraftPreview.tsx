/**
 * The live 3D preview of one draft, rendered by the host's material preview
 * widget from the draft's current file uses. Fills its parent, which must be
 * `position: relative` with a real size. Shows a hint while no file is assigned.
 */

import { useMemo, type ReactNode } from "react";
import type { PanelWidgets } from "@ldlework/toybox-sdk";
import type { MaterialDraft } from "../lib/draft.js";
import { draftMaps } from "../lib/entry.js";
import { previewUrlResolver } from "./previewSource.js";
import type { ThumbLoader } from "./thumbs.js";

const PREVIEW_SIZE = 1024;

interface Props {
  controls?: ReactNode;
  draft: MaterialDraft;
  loader: ThumbLoader;
  Preview: PanelWidgets["MaterialPreview"];
}

export function DraftPreview({ draft, loader, Preview, controls }: Props) {
  // A rename keeps `files`, so it does not reload the preview.
  const { maps, flipNormalGreen } = useMemo(() => draftMaps(draft), [draft.files]);
  const resolveUrl = useMemo(
    () => previewUrlResolver(draft, (rel) => loader.load(rel, PREVIEW_SIZE)),
    [draft.files, loader],
  );

  if (maps.length === 0) {
    return (
      <div
        className="export__hint"
        style={{
          position: "absolute",
          inset: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        Assign a texture to see the preview.
      </div>
    );
  }

  // Two drafts can hold the same file names; their paths tell them apart.
  const sourceKey = draft.files.map((f) => f.relPath).join("|");
  return (
    <Preview
      maps={maps}
      flipNormalGreen={flipNormalGreen}
      resolveUrl={resolveUrl}
      sourceKey={sourceKey}
      controls={controls}
    />
  );
}
