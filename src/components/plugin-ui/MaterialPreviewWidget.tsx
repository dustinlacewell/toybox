/**
 * The host's 3D material preview as a panel widget (`ctx.widgets.MaterialPreview`).
 * Builds a material from the plugin's maps and renders it in a MaterialStudio.
 * The material rebuilds when the maps or the flip flag change; a new
 * `resolveUrl` function identity does not.
 */

import type { MaterialPreviewProps } from "@ldlework/toybox-sdk";
import type { MaterialMap } from "../../domain/material";
import { buildMaterial } from "../../services/materialLoad";
import { MaterialStudio } from "../material/MaterialStudio";
import { useMaterial } from "../material/useMaterial";

export function MaterialPreviewWidget({ maps, flipNormalGreen = false, resolveUrl }: MaterialPreviewProps) {
  const key = JSON.stringify({ maps, flipNormalGreen });
  const state = useMaterial(key, () =>
    buildMaterial({ maps: maps.map(toDomainMap), flipNormalGreen, resolveUrl }),
  );
  return <MaterialStudio state={state} />;
}

function toDomainMap(map: MaterialPreviewProps["maps"][number]): MaterialMap {
  return { ...map, invert: map.invert ?? false };
}
