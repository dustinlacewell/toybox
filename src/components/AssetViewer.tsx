/**
 * Interactive 3D viewer for a single asset. Mounts an r3f <Canvas> only while an
 * asset is open (so the grid never pays for a live GL context). The model is
 * shown at its authored glTF origin (not recentered) over an infinite origin
 * grid with RGB axis arrows, so its true pivot is visible. The camera auto-frames
 * the model; the grid/axes stay origin-anchored. GPU resources are disposed when
 * the viewer closes or switches asset.
 */

import { useEffect, useRef, useState } from "react";
import { Canvas } from "@react-three/fiber";
import { OrbitControls } from "@react-three/drei";
import type { AnimationClip, Group } from "three";

import type { Asset } from "../domain/catalog";
import { loadAssetScene } from "../services/gltfLoad";
import { disposeScene } from "../services/disposeScene";
import { revealAsset } from "../services/reveal";
import { recenterAsset, type Align, type Axis } from "../services/tauriApi";
import { Spinner } from "../ds/Spinner";
import { Button } from "../ds/Button";
import { useFavorites } from "./useFavorites";
import { OriginGrid } from "./viewer/OriginGrid";
import { OriginAxes } from "./viewer/OriginAxes";
import { GeometryCenterMarker } from "./viewer/GeometryCenterMarker";
import { BoundingBoxWire } from "./viewer/BoundingBoxWire";
import { ViewerCamera, type OrbitTarget } from "./viewer/ViewerCamera";
import { ViewerToolbar } from "./viewer/ViewerToolbar";
import { AlignTools } from "./viewer/AlignTools";
import { AnimationBar } from "./viewer/AnimationBar";
import { AnimationDriver } from "./viewer/AnimationDriver";
import { useAnimation } from "./viewer/useAnimation";
import { defaultWidgetVisibility, type WidgetKey } from "./viewer/widgets";
import { measureExtent, type ModelExtent } from "./viewer/modelExtent";
import "./AssetViewer.css";

interface Props {
  asset: Asset;
  onClose: () => void;
}

type LoadState =
  | { kind: "loading" }
  | { kind: "ready"; group: Group; extent: ModelExtent; clips: AnimationClip[] }
  | { kind: "error"; message: string };

/** Stable empty-clip reference so useAnimation's deps don't churn when idle. */
const EMPTY_CLIPS: AnimationClip[] = [];

export function AssetViewer({ asset, onClose }: Props) {
  const [reloadNonce, setReloadNonce] = useState(0);
  const state = useAssetScene(asset, reloadNonce);
  const { toggleFavorite } = useFavorites();
  const [orbitTarget, setOrbitTarget] = useState<OrbitTarget>("geometry");
  const [widgets, setWidgets] = useState(defaultWidgetVisibility);
  const [aligning, setAligning] = useState(false);
  const toggleWidget = (key: WidgetKey) =>
    setWidgets((w) => ({ ...w, [key]: !w[key] }));

  const ready = state.kind === "ready" ? state : null;
  const anim = useAnimation(ready?.group ?? null, ready?.clips ?? EMPTY_CLIPS);

  const onAlign = async (axis: Axis, align: Align) => {
    setAligning(true);
    try {
      await recenterAsset(asset.id, axis, align);
      // The source .gltf changed; reload to show the corrected placement.
      setReloadNonce((n) => n + 1);
    } finally {
      setAligning(false);
    }
  };

  return (
    <div className="viewer" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="viewer__panel">
        <header className="viewer__header">
          <div>
            <div className="viewer__name">{asset.name}</div>
            <div className="viewer__meta">
              {asset.pack} · {asset.category}
            </div>
          </div>
          <div className="viewer__actions">
            <Button
              variant="ghost"
              onClick={() => toggleFavorite(asset.id)}
              title="Favorite"
            >
              {asset.user.favorite ? "★ Favorited" : "☆ Favorite"}
            </Button>
            <Button variant="ghost" onClick={() => void revealAsset(asset)}>
              Reveal in Explorer
            </Button>
            <button className="viewer__close" onClick={onClose} title="Close">
              ✕
            </button>
          </div>
        </header>

        <div className="viewer__stage">
          {state.kind === "ready" && (
            <>
              <ViewerToolbar
                visibility={widgets}
                onToggle={toggleWidget}
                orbitTarget={orbitTarget}
                onCycleOrbitTarget={() =>
                  setOrbitTarget((t) => (t === "geometry" ? "origin" : "geometry"))
                }
              />
              <AlignTools onAlign={onAlign} busy={aligning} />
              <AnimationBar controls={anim} />
            </>
          )}
          <Canvas camera={{ position: [3, 2, 4], fov: 45 }} dpr={[1, 2]}>
            <ambientLight intensity={0.5} />
            <hemisphereLight args={["#cfd8e3", "#2b2f38", 0.8]} />
            <directionalLight position={[5, 8, 5]} intensity={1.2} />
            <directionalLight position={[-4, 2, -3]} intensity={0.4} />
            {state.kind === "ready" && (
              <>
                {/* Model at its authored local origin (no recentering) so its
                    true placement relative to the origin is visible. */}
                <primitive object={state.group} />

                {/* Reference widgets — toggled from the viewer toolbar. */}
                {widgets.grid && <OriginGrid modelSize={state.extent.size} />}
                {widgets.axes && <OriginAxes length={state.extent.radius} />}
                {widgets.geometryCenter && (
                  <GeometryCenterMarker
                    position={state.extent.center}
                    scale={state.extent.radius}
                  />
                )}
                {widgets.boundingBox && (
                  <BoundingBoxWire min={state.extent.min} max={state.extent.max} />
                )}

                <ViewerCamera
                  center={state.extent.center}
                  radius={state.extent.radius}
                  target={orbitTarget}
                  frameKey={asset.id}
                />
                <AnimationDriver tick={anim.tick} />
              </>
            )}
            <OrbitControls makeDefault enableDamping />
          </Canvas>

          {state.kind === "loading" && (
            <div className="viewer__loading">
              <Spinner />
            </div>
          )}
          {state.kind === "error" && (
            <div className="viewer__error">Failed to load: {state.message}</div>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * Imperatively load the asset's scene; dispose its GPU resources on swap/close.
 * `reloadNonce` forces a re-load after the source .gltf is edited in place. A
 * reload (same asset, new nonce) swaps the model without flashing the spinner or
 * disturbing the camera — the previous group stays on screen until the new one
 * is ready, then the old one is disposed. Only an asset change shows the loading
 * state. The displayed group is tracked in a ref so disposal is unambiguous.
 */
function useAssetScene(asset: Asset, reloadNonce: number): LoadState {
  const [state, setState] = useState<LoadState>({ kind: "loading" });
  const displayed = useRef<Group | null>(null);

  useEffect(() => {
    let alive = true;

    // Spinner only when switching assets; an in-place reload swaps silently.
    setState((prev) => (prev.kind === "ready" ? prev : { kind: "loading" }));

    loadAssetScene(asset, reloadNonce).then(
      ({ scene, clips }) => {
        if (!alive) {
          disposeScene(scene);
          return;
        }
        const old = displayed.current;
        displayed.current = scene;
        if (old && old !== scene) disposeScene(old);
        setState({ kind: "ready", group: scene, extent: measureExtent(scene), clips });
      },
      (err) => alive && setState({ kind: "error", message: String(err) }),
    );

    return () => {
      alive = false;
    };
  }, [asset.id, reloadNonce]);

  // Dispose the displayed group when the viewer unmounts entirely.
  useEffect(() => {
    return () => {
      if (displayed.current) disposeScene(displayed.current);
      displayed.current = null;
    };
  }, []);

  return state;
}
