/**
 * One material in the review list: an include checkbox, a preview, the name over
 * the roles its files supply, and a "!" when it has problems. Clicking selects it.
 */

import { Checkbox } from "@ldlework/toybox-sdk/ui";
import type { ReviewItem } from "./edit.js";
import { previewFile, roleLabels } from "./summary.js";
import { Thumb } from "./Thumb.js";
import type { ThumbLoader } from "./thumbs.js";

interface Props {
  item: ReviewItem;
  problems: string[];
  selected: boolean;
  loader: ThumbLoader;
  onSelect: () => void;
  onInclude: (included: boolean) => void;
}

const ONE_LINE = { overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } as const;

export function MaterialRow({ item, problems, selected, loader, onSelect, onInclude }: Props) {
  const { draft, included } = item;
  return (
    <div
      onClick={onSelect}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "6px 10px",
        cursor: "pointer",
        background: selected ? "var(--accent-soft)" : undefined,
        opacity: included ? 1 : 0.5,
        contentVisibility: "auto",
        containIntrinsicSize: "auto 56px",
      }}
    >
      <span onClick={(e) => e.stopPropagation()} style={{ display: "flex" }}>
        <Checkbox checked={included} onChange={onInclude} label={null} />
      </span>
      <Thumb loader={loader} relPath={previewFile(draft)?.relPath} box={44} size={96} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ ...ONE_LINE, fontSize: "var(--fs-md)", color: "var(--text-0)" }}>
          {draft.name}
        </div>
        <div style={{ ...ONE_LINE, fontSize: "var(--fs-sm)", color: "var(--text-2)" }}>
          {roleLabels(draft).join(" · ")}
        </div>
      </div>
      {included && problems.length > 0 && (
        <span
          title={problems.join("\n")}
          style={{ flex: "none", fontWeight: 700, color: "var(--danger)" }}
        >
          !
        </span>
      )}
    </div>
  );
}
