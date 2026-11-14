/**
 * Load a material asset into a three.js material and own its disposal. The
 * material and its textures are disposed once when the asset changes or the
 * viewer unmounts; a load that resolves after that is disposed on arrival. The
 * shared studio environment is never touched.
 */

import { useEffect, useState } from "react";
import type { MeshStandardMaterial } from "three";

import type { MaterialAsset } from "../../domain/catalog";
import { loadMaterial } from "../../services/materialLoad";
import { disposeMaterial } from "../../services/materialTextures";

export type MaterialState =
  | { kind: "loading" }
  | { kind: "ready"; material: MeshStandardMaterial }
  | { kind: "error"; message: string };

type Settled = { forId: string; state: MaterialState };

const LOADING: MaterialState = { kind: "loading" };

export function useMaterial(asset: MaterialAsset): MaterialState {
  const [settled, setSettled] = useState<Settled | null>(null);

  useEffect(() => {
    let alive = true;
    let loaded: MeshStandardMaterial | null = null;

    loadMaterial(asset).then(
      (material) => {
        if (!alive) {
          disposeMaterial(material);
          return;
        }
        loaded = material;
        setSettled({ forId: asset.id, state: { kind: "ready", material } });
      },
      (err) => {
        if (alive) setSettled({ forId: asset.id, state: { kind: "error", message: String(err) } });
      },
    );

    return () => {
      alive = false;
      if (loaded) disposeMaterial(loaded);
    };
    // Reload only when a different asset opens, not when its metadata changes.
  }, [asset.id]);

  // A result for a previous asset (already disposed) is never handed out.
  return settled && settled.forId === asset.id ? settled.state : LOADING;
}
