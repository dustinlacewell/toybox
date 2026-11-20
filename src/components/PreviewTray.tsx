/**
 * The preview tray at the bottom of the library, between the side bars. Collapsed it is one line: the item (or the
 * selected items) and buttons to open the tray or the full view. Open it holds the inspector: the live preview
 * beside the item's status card. The grid keeps room for it under its last row (`--tray-space`), so scrolling
 * reaches everything.
 */

import type { ReactNode } from "react";
import { ChevronDown, ChevronUp, Maximize2 } from "lucide-react";
import { variantInfo } from "@ldlework/toybox-sdk";

import type { Asset } from "../domain/catalog";
import { provenanceValue } from "../domain/facets";
import { assetTypeEntry } from "../state/assetTypes";
import { useAssets } from "../state/assetCache";
import { useThumbUrl } from "./useThumbUrl";
import "./PreviewTray.css";

interface Props {
  /** The item the tray is about. */
  asset?: Asset;
  /** Selected items, when more than one is. */
  selected: string[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Open the full view of the item. */
  onExpand: () => void;
  /** The inspector's content, laid out by the tray when it is open. */
  children: ReactNode;
}

export function PreviewTray({ asset, selected, open, onOpenChange, onExpand, children }: Props) {
  const several = selected.length > 1;
  const stack = useAssets(several ? selected.slice(0, 4) : undefined).assets;
  if (!asset && !several) return null;
  const toggle = <button type="button" className="ds-icon-button" aria-label={open ? "Collapse the preview" : "Open the preview"} title={open ? "Collapse" : "Open the preview"} onClick={() => onOpenChange(!open)}>{open ? <ChevronDown size={16} /> : <ChevronUp size={16} />}</button>;
  const expand = asset && <button type="button" className="ds-icon-button" aria-label="Open the full view" title="Open the full view" onClick={onExpand}><Maximize2 size={15} /></button>;
  return <section className={`preview-tray${open ? " is-open" : ""}`} aria-label="Preview">
    <div className="preview-tray__bar" onDoubleClick={() => onOpenChange(!open)}>
      {several ? <>
        <span className="preview-tray__stack">{stack.map((item) => <Mini key={item.id} asset={item} />)}</span>
        <span className="preview-tray__name"><b>{selected.length.toLocaleString()} selected</b><span>{typeMix(stack, selected.length)}</span></span>
      </> : asset && <>
        <Mini asset={asset} />
        <span className="preview-tray__name"><b>{variantInfo(asset.name, asset.kind).base}</b><span>{[assetTypeEntry(asset.kind === "plugin" ? asset.type : asset.kind).singular, provenanceValue(asset, "publisher") || provenanceValue(asset, "source")].filter(Boolean).join(" · ")}</span></span>
      </>}
      <span className="preview-tray__spacer" />
      {toggle}{expand}
    </div>
    {open && asset && <div className="preview-tray__body">{children}</div>}
  </section>;
}

function Mini({ asset }: { asset: Asset }) {
  const url = useThumbUrl(asset);
  return <span className="preview-tray__mini">{url ? <img src={url} alt="" /> : null}</span>;
}

/** "2 models · 1 material", from the items at hand (the first few of a large selection). */
function typeMix(assets: Asset[], total: number): string {
  if (!assets.length) return "";
  const counts = new Map<string, number>();
  for (const asset of assets) {
    const type = asset.kind === "plugin" ? asset.type : asset.kind;
    counts.set(type, (counts.get(type) ?? 0) + 1);
  }
  if (counts.size === 1 && assets.length === total) return `${total} ${assetTypeEntry([...counts.keys()][0]).label.toLowerCase()}`;
  return counts.size === 1 ? `${assetTypeEntry([...counts.keys()][0]).label} so far` : "Several types";
}

