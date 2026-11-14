/**
 * The review screen: a large three-column dialog. The left column lists the
 * materials (filterable, with include-all); the centre shows the selected one in
 * 3D; the right column edits it. Holds only view state — which item is open and
 * which filter is on. The edits themselves live in the panel, so closing the
 * dialog keeps them.
 */

import { useMemo, useState } from "react";
import type { PanelWidgets } from "@ldlework/toybox-sdk";
import { Button, Checkbox, Dialog, Segmented, Stack } from "@ldlework/toybox-sdk/ui";
import { DraftPreview } from "./DraftPreview.js";
import { canImport, importable, problemsFor, type ReviewItem } from "./edit.js";
import type { ReviewHandlers } from "./handlers.js";
import { MaterialEditor } from "./MaterialEditor.js";
import { MaterialList } from "./MaterialList.js";
import { attentionIndexes } from "./summary.js";
import type { ThumbLoader } from "./thumbs.js";

type Filter = "all" | "attention";

interface Props extends ReviewHandlers {
  items: ReviewItem[];
  pack: string;
  category: string;
  loader: ThumbLoader;
  /** The host's 3D material preview. */
  Preview: PanelWidgets["MaterialPreview"];
  /** The item first shown. */
  initialSelected: number;
  onClose: () => void;
  onImport: () => void;
}

export function ReviewDialog({
  items,
  pack,
  category,
  loader,
  Preview,
  initialSelected,
  onClose,
  onImport,
  onInclude,
  onIncludeAll,
  onRename,
  onUse,
  onChannel,
}: Props) {
  const [selected, setSelected] = useState(initialSelected);
  const [filter, setFilter] = useState<Filter>("all");

  const problems = useMemo(
    () => items.map((_, i) => problemsFor(items, i, pack, category)),
    [items, pack, category],
  );
  const attention = useMemo(() => attentionIndexes(items, pack, category), [items, pack, category]);
  const count = useMemo(() => importable(items, pack, category).length, [items, pack, category]);

  const listed = useMemo(
    () => (filter === "attention" ? attention : items.map((_, i) => i)),
    [filter, attention, items],
  );
  const allListedIncluded = listed.length > 0 && listed.every((i) => items[i].included);
  const ready = canImport(items, pack, category);
  const open = items[selected];

  return (
    <Dialog
      open
      title="Review materials"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!ready} onClick={onImport}>
            Import {count} into library
          </Button>
        </>
      }
    >
      <div style={{ display: "flex", flex: 1, minWidth: 0, minHeight: 0 }}>
        <div
          style={{
            width: 300,
            flex: "none",
            display: "flex",
            flexDirection: "column",
            borderRight: "1px solid var(--border)",
          }}
        >
          <Stack dir="row" gap={12} align="center" justify="space-between" style={{ padding: 10 }}>
            <Segmented
              value={filter}
              options={[
                { value: "all", label: `All (${items.length})` },
                { value: "attention", label: `Needs attention (${attention.length})` },
              ]}
              onChange={setFilter}
            />
            <Checkbox
              checked={allListedIncluded}
              onChange={(included) => onIncludeAll(listed, included)}
              label="Include all"
            />
          </Stack>
          <MaterialList
            items={items}
            problems={problems}
            indexes={listed}
            selected={selected}
            loader={loader}
            onSelect={setSelected}
            onInclude={onInclude}
          />
        </div>
        <div style={{ flex: 1, minWidth: 0, position: "relative" }}>
          {open && <DraftPreview draft={open.draft} loader={loader} Preview={Preview} />}
        </div>
        <div
          style={{
            width: 320,
            flex: "none",
            overflowY: "auto",
            padding: 12,
            borderLeft: "1px solid var(--border)",
          }}
        >
          {open ? (
            <MaterialEditor
              item={open}
              problems={problems[selected]}
              loader={loader}
              onRename={(name) => onRename(selected, name)}
              onUse={(fileIndex, use) => onUse(selected, fileIndex, use)}
              onChannel={(fileIndex, c, role) => onChannel(selected, fileIndex, c, role)}
            />
          ) : (
            <div className="export__hint">No materials found.</div>
          )}
        </div>
      </div>
    </Dialog>
  );
}
