import { Select } from "@ldlework/toybox-sdk/ui";
import type { useCompatiblePreviews } from "./useCompatiblePreviews";
import { sourceLabel } from "../../domain/facets";

interface Props { kind: "model" | "animation"; preview: ReturnType<typeof useCompatiblePreviews>; }
export function PreviewPicker({ kind, preview }: Props) {
  return <div className="viewer-preview-picker">
    <label><span className="viewer-panel-section">Preview {kind}</span>
      <Select size={kind==="animation"?8:undefined} className={kind==="animation"?"viewer-preview-picker__list":undefined} value={preview.value} options={[
        { value: "", label: kind === "model" ? "Skeleton only" : "Rest pose" },
        ...preview.choices.map(choice => ({ value: choice.assetId, label: `${choice.asset.name} · ${choice.asset.kind === "animation" ? `${choice.asset.clip.sourceName} · ` : ""}${sourceLabel(choice.asset)}` })),
      ]} onChange={value => void preview.select(value)} disabled={preview.loading} />
    </label>
    <p className="viewer-panel-subtitle" role="status">{preview.error ?? (preview.loading ? "Finding compatible rigs…" : preview.selected ? preview.selected.retargeted ? "Retargeted preview" : preview.selected.exact ? "Same rig · direct playback" : "Compatible tracks · direct playback" : preview.choices.length ? `${preview.choices.length} compatible ${kind}${preview.choices.length === 1 ? "" : "s"}` : `No compatible ${kind === "model" ? "models" : "animations"} in this library.`)}</p>
  </div>;
}
