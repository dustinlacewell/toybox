import { Checkbox, NavigationItem } from "@ldlework/toybox-sdk/ui";
import type { ReviewItem } from "./edit.js";
import { previewFile, roleLabels } from "./summary.js";
import { Thumb } from "./Thumb.js";
import type { ThumbLoader } from "./thumbs.js";

interface Props { item: ReviewItem; problems: string[]; selected?: boolean; loader: ThumbLoader; onSelect?: () => void; onOpen?: () => void; onInclude: (included: boolean) => void; }
export function MaterialRow({ item, problems, selected, loader, onSelect, onOpen, onInclude }: Props) {
  return <div onDoubleClick={onOpen} style={{ display: "flex", alignItems: "center", gap: 4, opacity: item.included ? 1 : 0.5 }}>
    <Checkbox checked={item.included} onChange={onInclude} label={null} />
    <NavigationItem active={selected} onPress={onSelect ?? onOpen ?? (() => {})} detail={problems.length ? <span title={problems.join("\n")} style={{ color: "#e5a23c" }}>!</span> : undefined}
      label={<span style={{ display: "flex", alignItems: "center", gap: 10 }}><Thumb loader={loader} relPath={previewFile(item.draft)?.relPath} box={32} size={96} /><span style={{ minWidth: 0 }}><span style={{ display: "block", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{item.draft.name}</span><small style={{ display: "block", color: "var(--text-2)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{roleLabels(item.draft).join(" · ")}</small></span></span>} />
  </div>;
}
