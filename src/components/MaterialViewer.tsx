import { useState } from "react";
import { NavigationItem, PropertyRow, Segmented, ThumbnailStrip, type ImageChannel, type ImageScale } from "@ldlework/toybox-sdk/ui";
import { variantInfo, MATERIAL_ROLE_LABELS } from "@ldlework/toybox-sdk";
import type { MaterialAsset } from "../domain/catalog";
import { loadMaterial } from "../services/materialLoad";
import { ViewerFrame } from "./ViewerFrame";
import { MapImage } from "./material/MapImage";
import { MaterialStudio } from "./material/MaterialStudio";
import { useMaterial } from "./material/useMaterial";
import { VariantSwitch } from "./VariantSwitch";
import { FileThumbnail } from "./material/FileThumbnail";
import "./MaterialViewer.css";

interface Props { asset: MaterialAsset; onClose: () => void; onExport?: () => void; }
const CHANNELS = [{ value: "all", label: "All" }, { value: "r", label: "R" }, { value: "g", label: "G" }, { value: "b", label: "B" }, { value: "a", label: "A" }];
export function MaterialViewer({ asset, onClose, onExport }: Props) {
  const [variant, setVariant] = useState(0);
  const [selected, setSelected] = useState(-1);
  const [channel, setChannel] = useState<ImageChannel>("all");
  const [scale, setScale] = useState<ImageScale>("fit");
  const variants = [asset, ...(asset.variants ?? []).filter((a): a is MaterialAsset => a.kind === "material")];
  const active = variants[variant] ?? asset;
  const state = useMaterial(active.id, () => loadMaterial(active), true);
  const maps = active.material.maps;
  const map = maps[selected];
  const files = new Set(maps.map((m) => m.file));
  const pick = (index: number) => { setSelected(index); setChannel("all"); };
  return <ViewerFrame asset={asset} onClose={onClose} onExport={onExport} sidebarLabel="Material" sidebar={<>
    <h2 className="viewer-panel-title">{map ? MATERIAL_ROLE_LABELS[map.role] : "Material"}</h2>
    <p className="viewer-panel-subtitle">{map ? "One texture, shown alone" : `${maps.length} maps from ${files.size} files`}</p>
    {map ? <><PropertyRow label="File" value={map.file.split(/[\\/]/).pop()} /><PropertyRow label="Used as" value={MATERIAL_ROLE_LABELS[map.role]} /><PropertyRow label="Channel" value={map.channel?.toUpperCase() ?? "Whole image"} /><PropertyRow label="Inverted" value={map.invert ? "Yes" : "No"} /><PropertyRow label="Shared with" value={`${maps.filter((m) => m.file === map.file).length - 1} other maps`} /></>
      : maps.map((m, i) => <NavigationItem key={i} label={<span>{MATERIAL_ROLE_LABELS[m.role]}<small className="material-file-label">{m.file.split(/[\\/]/).pop()}{m.channel ? `, ${m.channel.toUpperCase()}` : ""}</small></span>} onPress={() => pick(i)} />)}
    <div className="viewer-panel-section">Sizes</div><PropertyRow label="Current size" value={variantInfo(active.name, "material").label} /><PropertyRow label="Available sizes" value={variants.length} />
  </>} footer={<ThumbnailStrip label="Material maps" value={String(selected)} onChange={(value) => pick(Number(value))} options={[
    { value: "-1", label: "Material", preview: <span className="material-result-glyph">◉</span> },
    ...maps.map((m, i) => ({ value: String(i), label: MATERIAL_ROLE_LABELS[m.role], preview: <FileThumbnail library={asset.library} file={m.file} /> })),
  ]} />}>
    <MaterialStudio state={state} active={!map} controls={variants.length > 1 && <VariantSwitch variants={variants} selected={variant} onSelect={(i) => { setVariant(i); setSelected(-1); }} />}/>
    {map && <MapImage library={asset.library} file={map.file} channel={channel} scale={scale} />}
    {map && <div className="viewer__variants">
      {map && <><Segmented value={channel} options={CHANNELS} onChange={(v) => setChannel(v as ImageChannel)} /><Segmented value={scale} options={[{ value: "fit", label: "Fit" }, { value: "actual", label: "1:1" }]} onChange={setScale} /></>}
      {variants.length > 1 && <VariantSwitch variants={variants} selected={variant} onSelect={(i) => { setVariant(i); setSelected(-1); }} />}
    </div>}
  </ViewerFrame>;
}
