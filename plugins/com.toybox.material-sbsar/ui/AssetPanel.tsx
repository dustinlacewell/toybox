/**
 * The Substance tab of a material this plugin made: which substance it was
 * rendered from, with which preset, size and changed parameters, and a way to
 * open that substance with this material chosen for editing. Read-only, and
 * the engine is not started here: editing happens in the substance's viewer.
 */

import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { Button, PropertyRow } from "@ldlework/toybox-sdk/ui";
import { defineAssetPanel, type AssetPanelCtx, type AssetView } from "@ldlework/toybox-sdk";

import { showValue, pixels } from "../lib/describe.js";
import { prettyIdentifier } from "../lib/editor.js";
import { readMeta } from "../lib/graph.js";
import { parentOf } from "../lib/rerender.js";
import { isLegacyMaterial, isSubstance } from "../lib/substance.js";
import { requestEdit } from "./handoff.js";

const note: CSSProperties = { color: "var(--text-1)", margin: "0 8px 12px", lineHeight: 1.5 };
const card: CSSProperties = { display: "flex", alignItems: "center", gap: 12, padding: "8px", minWidth: 0 };

export default defineAssetPanel(function SubstanceTab({ ctx }: { ctx: AssetPanelCtx }) {
  const { asset, host, widgets } = ctx;
  const meta = useMemo(() => readMeta(asset.origin?.meta), [asset.origin]);
  const link = useMemo(() => parentOf(asset), [asset]);
  const [parent, setParent] = useState<AssetView | null>();
  useEffect(() => {
    if (!("id" in link)) return;
    let alive = true;
    setParent(undefined);
    host.getAssetAsync(link.id).then((found) => { if (alive) setParent(found && isSubstance(found) ? found : null); }, () => { if (alive) setParent(null); });
    return () => { alive = false; };
  }, [host, link]);

  if (isLegacyMaterial(asset)) return <p style={note}>This material keeps its Substance archive the old way. The library is converted to substances the next time Toybox opens it; this tab then leads to the substance.</p>;
  if (!meta) return <p style={note}>This material records no Substance rendering.</p>;

  const open = () => {
    if (!parent) return;
    requestEdit(parent.id, asset.id);
    host.openAsset(parent.id);
  };
  const changed = Object.entries(meta.inputs);
  return <>
    {"reason" in link ? <p style={note}>Rendered from {meta.sbsar}, but {link.reason}.</p>
      : parent === null ? <p role="note" style={note}>Rendered from {meta.sbsar}. Its substance is no longer in the library, so it cannot be edited again.</p>
      : <div style={card}>
        <span style={{ width: 56, height: 56, flex: "none", borderRadius: 8, overflow: "hidden", background: "var(--bg-2)" }}>
          {parent && <widgets.AssetThumbnail id={parent.id} size={56} />}
        </span>
        <span style={{ display: "grid", gap: 2, minWidth: 0 }}>
          <strong style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{parent?.name ?? "Looking for its substance…"}</strong>
          <span style={{ color: "var(--text-2)", fontSize: 12, overflowWrap: "anywhere" }}>Substance · {meta.sbsar}</span>
        </span>
      </div>}
    <PropertyRow label="Preset" value={meta.preset ?? "None (defaults)"} />
    {meta.graphLabel && <PropertyRow label="Graph" value={meta.graphLabel} />}
    <PropertyRow label="Texture size" value={`${pixels(meta.size)} × ${pixels(meta.size)}`} />
    <div className="viewer-panel-section">{changed.length ? `Changed parameters (${changed.length})` : "Parameters"}</div>
    {changed.length ? changed.map(([key, value]) => <PropertyRow key={key} label={<span title={key}>{prettyIdentifier(key)}</span>} value={showValue(value)} />)
      : <p style={note}>{meta.preset ? "As the preset sets them." : "The archive's defaults."}</p>}
    {parent && <div style={{ padding: "16px 8px 8px", display: "grid" }}>
      <Button variant="primary" onClick={open}>Open substance</Button>
      <p style={{ ...note, margin: "8px 0 0", color: "var(--text-2)", fontSize: 12 }}>Opens {parent.name} with this material chosen, to change its preset, parameters or size.</p>
    </div>}
  </>;
});
