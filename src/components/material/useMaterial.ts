/**
 * Run a material load and own the result's disposal. The load re-runs when `key`
 * changes (using the latest `load`), not when `load`'s identity changes. The
 * material and its textures are disposed once when the key changes or the
 * viewer unmounts; a load that resolves after that is disposed on arrival. The
 * shared studio environment is never touched.
 */

import { useEffect, useRef, useState } from "react";
import type { MeshStandardMaterial } from "three";

import { disposeMaterial } from "../../services/materialTextures";

export type MaterialState =
  | { kind: "loading" }
  | { kind: "ready"; material: MeshStandardMaterial }
  | { kind: "error"; message: string };

type Settled = { forKey: string; state: MaterialState };

const LOADING: MaterialState = { kind: "loading" };

export function useMaterial(key: string, load: () => Promise<MeshStandardMaterial>, retainDuringLoad = false): MaterialState {
  const [settled, setSettled] = useState<Settled | null>(null);
  const displayed = useRef<MeshStandardMaterial | null>(null);
  const latestLoad = useRef(load);
  latestLoad.current = load;

  useEffect(() => {
    let alive = true;

    latestLoad.current().then(
      (material) => {
        if (!alive) {
          disposeMaterial(material);
          return;
        }
        const previous = displayed.current;
        displayed.current = material;
        setSettled({ forKey: key, state: { kind: "ready", material } });
        if (previous && previous !== material) disposeMaterial(previous);
      },
      (err) => {
        if (alive) setSettled({ forKey: key, state: { kind: "error", message: String(err) } });
      },
    );

    return () => {
      alive = false;
    };
  }, [key]);

  useEffect(() => () => {
    if (displayed.current) disposeMaterial(displayed.current);
    displayed.current = null;
  }, []);
  // Keeping the previous material while a size loads preserves the live camera.
  return settled && (settled.forKey === key || retainDuringLoad) ? settled.state : LOADING;
}
