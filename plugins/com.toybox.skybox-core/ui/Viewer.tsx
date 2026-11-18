/**
 * The full view of one skybox: look around inside it, set the exposure, and
 * check its lighting on a mirror and a matte sphere.
 */

import { useEffect, useMemo, useState } from "react";
import { Button, Checkbox, PropertyRow } from "@ldlework/toybox-sdk/ui";
import type { AssetViewerCtx } from "@ldlework/toybox-sdk";

import { LAYOUT_LABELS, describeSize, isHdr } from "../lib/detect.js";
import { librarySource } from "../lib/thumbnail.js";
import { SkyCanvas } from "./SkyCanvas.js";

export function SkyViewer({ ctx }: { ctx: AssetViewerCtx }) {
  const { asset, files, setSidebar } = ctx;
  const [exposure, setExposure] = useState(0);
  const [spheres, setSpheres] = useState(false);
  const source = useMemo(() => librarySource(asset, files), [asset, files]);
  const { meta } = source;

  useEffect(() => {
    setSidebar(<>
      <h2 className="viewer-panel-title">Sky</h2>
      <PropertyRow label="Layout" value={LAYOUT_LABELS[meta.layout]} />
      <PropertyRow label="Format" value={meta.format} />
      <PropertyRow label="Size" value={describeSize(meta)} />
      <PropertyRow label="Light range" value={isHdr(meta.encoding) ? "High dynamic range" : "Standard"} />
      <div className="viewer-panel-section">View</div>
      <label style={{ display: "grid", gap: 6, padding: "8px" }}>
        <span style={{ display: "flex", justifyContent: "space-between" }}><span>Exposure</span><span style={{ color: "var(--text-2)" }}>{exposure > 0 ? "+" : ""}{exposure.toFixed(1)} stops</span></span>
        <input type="range" min={-6} max={6} step={0.1} value={exposure} onChange={(event) => setExposure(Number(event.currentTarget.value))} />
      </label>
      <div style={{ padding: "0 8px 8px" }}><Button variant="ghost" disabled={exposure === 0} onClick={() => setExposure(0)}>Reset exposure</Button></div>
      <div style={{ padding: "0 8px" }}><Checkbox checked={spheres} label="Show lighting spheres" onChange={setSpheres} /></div>
      <p className="viewer-panel-subtitle" style={{ padding: "8px" }}>Drag to look around. Scroll to zoom.</p>
    </>);
    return () => setSidebar(null);
  }, [setSidebar, meta, exposure, spheres]);

  return <SkyCanvas sourceKey={asset.id} source={source} exposure={exposure} spheres={spheres} />;
}
